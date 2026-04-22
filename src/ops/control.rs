// ── Control flow operators ─────────────────────────────────────────────────────
//
// This module implements:
//
//   if        — conditional execution
//   ifelse    — two-branch conditional
//   repeat    — fixed-count loop
//   for       — numeric range loop
//   quit      — terminate the interpreter
//
// ── How { ... } blocks are pushed vs. executed ────────────────────────────────
//
// The short answer: a `{ ... }` literal is ALWAYS pushed onto the operand stack
// as a data value, never executed on sight.  Only explicit operator calls like
// `if` or `ifelse` decide to run the procedure.
//
// When the source program contains `true { 42 } if`, the execution loop:
//   1. Pushes `PSValue::Boolean(true)` onto the operand stack.
//   2. Sees `{ 42 }` and pushes `PSValue::Procedure(Rc<[Integer(42)]>, None)`.
//      The braces are NOT a signal to execute — they are a LITERAL, just like
//      parentheses around a string.
//   3. Sees the executable name `if`, looks it up in systemdict, and calls
//      `op_if`, which pops the proc and the bool, sees `true`, and calls
//      `interp.exec_proc(proc)`.
//
// `exec_proc` feeds the body tokens to `exec_body`, which dispatches them:
//   • Integer / Float / Boolean / String / Name / Null / Mark / Procedure
//         → pushed as data (same semantics as in the outer execution loop).
//   • Operator(f)
//         → f(interp) is called immediately.
//         → (Operator tokens appear in a body when it is built directly from Rust
//            code, as in tests.  In parsed programs they arrive via ExecutableName.)
//   • ExecutableName(name)
//         → looked up in the dict stack (or captured scope in lexical mode).
//         → If the result is a Procedure, it is executed recursively.
//         → If the result is an Operator, its closure is called.
//         → Any other result is pushed as data.
//         → Name not found → Err(Undefined).
//
// So the distinction is purely about the VARIANT of the token:
//   Procedure-as-token  →  always push (it's a literal value)
//   ExecutableName-as-token  →  look up and possibly execute
//
// `if` knows to run its argument because it explicitly calls `exec_proc` after
// popping the procedure; it does not look at the procedure body itself.

use std::rc::Rc;

use crate::interpreter::Interpreter;
use crate::types::{PSError, PSValue};

// ── helpers ────────────────────────────────────────────────────────────────────

fn pop_proc(interp: &mut Interpreter) -> Result<PSValue, PSError> {
    match interp.pop()? {
        v @ PSValue::Procedure(_, _) => Ok(v),
        other => {
            interp.push(other);
            Err(PSError::TypeCheck { expected: "procedure", got: "non-procedure" })
        }
    }
}

fn pop_bool(interp: &mut Interpreter) -> Result<bool, PSError> {
    match interp.pop()? {
        PSValue::Boolean(b) => Ok(b),
        other => {
            interp.push(other);
            Err(PSError::TypeCheck { expected: "boolean", got: "non-boolean" })
        }
    }
}

fn pop_nonneg_int(interp: &mut Interpreter) -> Result<i64, PSError> {
    match interp.pop()? {
        PSValue::Integer(n) if n >= 0 => Ok(n),
        PSValue::Integer(_) => Err(PSError::RangeCheck),
        other => {
            interp.push(other);
            Err(PSError::TypeCheck { expected: "non-negative integer", got: "non-integer" })
        }
    }
}

fn pop_number(interp: &mut Interpreter) -> Result<PSValue, PSError> {
    match interp.pop()? {
        v @ (PSValue::Integer(_) | PSValue::Float(_)) => Ok(v),
        other => {
            interp.push(other);
            Err(PSError::TypeCheck { expected: "number", got: "non-number" })
        }
    }
}

fn as_f64(v: &PSValue) -> f64 {
    match v {
        PSValue::Integer(n) => *n as f64,
        PSValue::Float(f)   => *f,
        _ => unreachable!("caller must have type-checked already"),
    }
}

// ── operators ──────────────────────────────────────────────────────────────────

/// `if` — conditionally execute a procedure.
///
/// Stack effect: `bool proc → `
///
/// Pops `proc` and `bool`.  Executes `proc` if `bool` is `true`; does nothing
/// if `bool` is `false`.  In both cases `proc` is consumed and removed from the
/// stack.
///
/// Errors:
///   `typecheck` if the top value is not a procedure (value is pushed back).
///   `typecheck` if the second value is not a boolean (value is pushed back).
pub fn op_if(interp: &mut Interpreter) -> Result<(), PSError> {
    let proc = pop_proc(interp)?;
    let cond = pop_bool(interp)?;
    if cond {
        interp.exec_proc(proc)?;
    }
    Ok(())
}

/// `ifelse` — execute one of two procedures based on a boolean.
///
/// Stack effect: `bool proc_true proc_false → `
///
/// Pops `proc_false` (top), `proc_true`, and `bool`.
/// Executes `proc_true` if `bool` is `true`; executes `proc_false` otherwise.
///
/// Stack order note: the *false* branch is on top because PostScript pushes
/// arguments left-to-right — `bool` was pushed first, `proc_true` second,
/// `proc_false` last (on top).
///
/// Errors: `typecheck` if either procedure slot is not a procedure, or if the
/// boolean slot is not a boolean.
pub fn op_ifelse(interp: &mut Interpreter) -> Result<(), PSError> {
    let proc_false = pop_proc(interp)?;
    let proc_true  = pop_proc(interp)?;
    let cond       = pop_bool(interp)?;
    if cond {
        interp.exec_proc(proc_true)?;
    } else {
        interp.exec_proc(proc_false)?;
    }
    Ok(())
}

/// `repeat` — execute a procedure a fixed number of times.
///
/// Stack effect: `int proc → `
///
/// Pops `proc` and `int`, then executes `proc` exactly `int` times in sequence.
/// If `int` is 0, `proc` is never called.  The loop counter is NOT pushed onto
/// the operand stack; use `for` if you need the counter value.
///
/// Errors:
///   `typecheck`  if `proc` is not a procedure.
///   `rangecheck` if `int` is negative.
///   `typecheck`  if `int` is not an integer.
pub fn op_repeat(interp: &mut Interpreter) -> Result<(), PSError> {
    let proc = pop_proc(interp)?;
    let n    = pop_nonneg_int(interp)?;
    for _ in 0..n {
        interp.exec_proc(proc.clone())?;
    }
    Ok(())
}

/// `for` — execute a procedure over a numeric range, exposing the counter.
///
/// Stack effect: `initial increment limit proc → `
///
/// Before each call to `proc`, the current counter value is pushed onto the
/// operand stack.  `proc` can use or ignore it.
///
/// Loop direction is determined by the sign of `increment`:
///   - `increment > 0`: run while `counter <= limit`.
///   - `increment < 0`: run while `counter >= limit`.
///   - `increment = 0`: body never executes (PostScript leaves this undefined,
///                       we choose "zero iterations" as the safest behaviour).
///
/// Type rules:
///   - All of `initial`, `increment`, `limit` are integers → integer loop.
///     The counter is pushed as `PSValue::Integer` each iteration.
///   - Any is a real → float loop.  All three are widened to `f64`; the counter
///     is pushed as `PSValue::Float` each iteration.
///
/// Errors:
///   `typecheck` if `proc` is not a procedure.
///   `typecheck` if any of the three numeric args is not a number.
pub fn op_for(interp: &mut Interpreter) -> Result<(), PSError> {
    let proc  = pop_proc(interp)?;
    let limit = pop_number(interp)?;
    let inc   = pop_number(interp)?;
    let init  = pop_number(interp)?;

    match (&init, &inc, &limit) {
        (PSValue::Integer(i), PSValue::Integer(step), PSValue::Integer(lim)) => {
            let (mut counter, step, lim) = (*i, *step, *lim);
            if step > 0 {
                while counter <= lim {
                    interp.push(PSValue::Integer(counter));
                    interp.exec_proc(proc.clone())?;
                    counter = counter.wrapping_add(step);
                }
            } else if step < 0 {
                while counter >= lim {
                    interp.push(PSValue::Integer(counter));
                    interp.exec_proc(proc.clone())?;
                    counter = counter.wrapping_add(step);
                }
            }
            // step == 0: zero iterations
        }
        _ => {
            let mut counter = as_f64(&init);
            let step        = as_f64(&inc);
            let lim         = as_f64(&limit);
            if step > 0.0 {
                while counter <= lim {
                    interp.push(PSValue::Float(counter));
                    interp.exec_proc(proc.clone())?;
                    counter += step;
                }
            } else if step < 0.0 {
                while counter >= lim {
                    interp.push(PSValue::Float(counter));
                    interp.exec_proc(proc.clone())?;
                    counter += step;
                }
            }
        }
    }
    Ok(())
}

/// `quit` — terminate the interpreter.
///
/// Stack effect: (none consumed, none produced)
///
/// Returns `Err(PSError::Quit)` to signal the outer execution loop to exit
/// cleanly.  The top-level runner should match on `PSError::Quit` and treat it
/// as a normal exit rather than an error.  Any values already on the operand
/// stack are left untouched.
pub fn op_quit(_interp: &mut Interpreter) -> Result<(), PSError> {
    Err(PSError::Quit)
}

// ── tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── test helpers ─────────────────────────────────────────────────────────
    //
    // Tests build procedure bodies directly from PSValue tokens rather than
    // going through the parser + systemdict registration.  This lets us embed
    // PSValue::Operator tokens (anonymous Rust closures) in a body to call
    // arithmetic operations without needing the full operator registry.

    fn int(n: i64)    -> PSValue { PSValue::Integer(n) }
    fn flt(f: f64)    -> PSValue { PSValue::Float(f) }
    fn bool_(b: bool) -> PSValue { PSValue::Boolean(b) }

    fn pop_int(i: &mut Interpreter) -> i64 {
        match i.pop().unwrap() {
            PSValue::Integer(n) => n,
            other => panic!("expected Integer, got: {other}"),
        }
    }

    fn pop_flt(i: &mut Interpreter) -> f64 {
        match i.pop().unwrap() {
            PSValue::Float(f) => f,
            other => panic!("expected Float, got: {other}"),
        }
    }

    /// Procedure whose body is a single integer literal.
    /// When executed via exec_proc, pushes that integer.
    fn push_int_proc(n: i64) -> PSValue {
        PSValue::Procedure(Rc::new(vec![int(n)]), None)
    }

    /// Empty procedure (body = []).  Used as the `for` body when we only want
    /// to observe the counter values the loop pushes automatically.
    fn empty_proc() -> PSValue {
        PSValue::Procedure(Rc::new(vec![]), None)
    }

    /// Procedure containing an Operator token that pops two integers and pushes
    /// their sum.  Demonstrates that Operator tokens inside a body are called.
    fn add_proc() -> PSValue {
        let op = Rc::new(|i: &mut Interpreter| -> Result<(), PSError> {
            let b = i.pop()?;
            let a = i.pop()?;
            match (a, b) {
                (PSValue::Integer(x), PSValue::Integer(y)) => {
                    i.push(PSValue::Integer(x + y));
                    Ok(())
                }
                _ => Err(PSError::TypeCheck { expected: "integer", got: "other" }),
            }
        });
        PSValue::Procedure(Rc::new(vec![PSValue::Operator(op)]), None)
    }

    // ── if ────────────────────────────────────────────────────────────────────

    #[test]
    fn if_true_executes_proc() {
        let mut i = Interpreter::new();
        i.push(bool_(true));
        i.push(push_int_proc(42));
        op_if(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 42);
        assert_eq!(i.operand_stack.len(), 0);
    }

    #[test]
    fn if_false_skips_proc() {
        let mut i = Interpreter::new();
        i.push(bool_(false));
        i.push(push_int_proc(42));
        op_if(&mut i).unwrap();
        assert_eq!(i.operand_stack.len(), 0);
    }

    #[test]
    fn if_non_proc_typecheck_restores_stack() {
        // If the top is not a procedure, it must be pushed back.
        let mut i = Interpreter::new();
        i.push(bool_(true));
        i.push(int(99));
        assert!(matches!(op_if(&mut i), Err(PSError::TypeCheck { .. })));
        assert_eq!(pop_int(&mut i), 99);
    }

    #[test]
    fn if_non_bool_typecheck_restores_stack() {
        let mut i = Interpreter::new();
        i.push(int(1));
        i.push(push_int_proc(0));
        assert!(matches!(op_if(&mut i), Err(PSError::TypeCheck { .. })));
    }

    #[test]
    fn if_operator_token_in_body_is_called() {
        // Stack before: 10 20 true { add_op }
        // The operator token inside the proc must be called, not pushed.
        // Expected after: 30
        let mut i = Interpreter::new();
        i.push(int(10));
        i.push(int(20));
        i.push(bool_(true));
        i.push(add_proc());
        op_if(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 30);
        assert_eq!(i.operand_stack.len(), 0);
    }

    // ── ifelse ────────────────────────────────────────────────────────────────

    #[test]
    fn ifelse_true_branch() {
        let mut i = Interpreter::new();
        i.push(bool_(true));
        i.push(push_int_proc(1));
        i.push(push_int_proc(2));
        op_ifelse(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 1);
        assert_eq!(i.operand_stack.len(), 0);
    }

    #[test]
    fn ifelse_false_branch() {
        let mut i = Interpreter::new();
        i.push(bool_(false));
        i.push(push_int_proc(1));
        i.push(push_int_proc(2));
        op_ifelse(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 2);
        assert_eq!(i.operand_stack.len(), 0);
    }

    #[test]
    fn ifelse_non_bool_typecheck() {
        let mut i = Interpreter::new();
        i.push(int(0));
        i.push(push_int_proc(1));
        i.push(push_int_proc(2));
        assert!(matches!(op_ifelse(&mut i), Err(PSError::TypeCheck { .. })));
    }

    #[test]
    fn ifelse_non_proc_false_branch_typecheck() {
        let mut i = Interpreter::new();
        i.push(bool_(true));
        i.push(push_int_proc(1));
        i.push(int(99)); // not a proc on top
        assert!(matches!(op_ifelse(&mut i), Err(PSError::TypeCheck { .. })));
        assert_eq!(pop_int(&mut i), 99);
    }

    // ── repeat ────────────────────────────────────────────────────────────────

    #[test]
    fn repeat_zero_times() {
        let mut i = Interpreter::new();
        i.push(int(0));
        i.push(push_int_proc(99));
        op_repeat(&mut i).unwrap();
        assert_eq!(i.operand_stack.len(), 0);
    }

    #[test]
    fn repeat_three_times() {
        let mut i = Interpreter::new();
        i.push(int(3));
        i.push(push_int_proc(7));
        op_repeat(&mut i).unwrap();
        assert_eq!(i.operand_stack.len(), 3);
        assert_eq!(pop_int(&mut i), 7);
        assert_eq!(pop_int(&mut i), 7);
        assert_eq!(pop_int(&mut i), 7);
    }

    #[test]
    fn repeat_accumulates_with_operator() {
        // Start with 0; repeat 4 times a proc that pushes 1 then adds.
        // After each iteration: acc += 1.  Final result: 4.
        let add1 = Rc::new(|i: &mut Interpreter| -> Result<(), PSError> {
            let b = i.pop()?;
            let a = i.pop()?;
            match (a, b) {
                (PSValue::Integer(x), PSValue::Integer(y)) => {
                    i.push(PSValue::Integer(x + y));
                    Ok(())
                }
                _ => Err(PSError::TypeCheck { expected: "integer", got: "other" }),
            }
        });
        let proc = PSValue::Procedure(
            Rc::new(vec![PSValue::Integer(1), PSValue::Operator(add1)]),
            None,
        );
        let mut i = Interpreter::new();
        i.push(int(0)); // accumulator
        i.push(int(4)); // repeat count
        i.push(proc);
        op_repeat(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 4);
        assert_eq!(i.operand_stack.len(), 0);
    }

    #[test]
    fn repeat_negative_rangecheck() {
        let mut i = Interpreter::new();
        i.push(int(-1));
        i.push(push_int_proc(0));
        assert!(matches!(op_repeat(&mut i), Err(PSError::RangeCheck)));
    }

    #[test]
    fn repeat_non_integer_typecheck() {
        let mut i = Interpreter::new();
        i.push(flt(2.0));
        i.push(push_int_proc(0));
        assert!(matches!(op_repeat(&mut i), Err(PSError::TypeCheck { .. })));
    }

    #[test]
    fn repeat_non_proc_typecheck() {
        let mut i = Interpreter::new();
        i.push(int(3));
        i.push(int(99));
        assert!(matches!(op_repeat(&mut i), Err(PSError::TypeCheck { .. })));
    }

    // ── for ───────────────────────────────────────────────────────────────────

    /// Helper: run `for` with an empty body and collect all counter values the
    /// loop pushes onto the stack.
    fn collect_for_counters(init: PSValue, inc: PSValue, lim: PSValue) -> Vec<PSValue> {
        let mut i = Interpreter::new();
        i.push(init);
        i.push(inc);
        i.push(lim);
        i.push(empty_proc());
        op_for(&mut i).unwrap();
        let mut results = Vec::new();
        while i.operand_stack.len() > 0 {
            results.push(i.pop().unwrap());
        }
        results.reverse(); // pop gives top-first; reverse to get bottom-first
        results
    }

    #[test]
    fn for_int_ascending() {
        let vals = collect_for_counters(int(1), int(1), int(3));
        assert_eq!(vals.len(), 3);
        assert!(matches!(vals[0], PSValue::Integer(1)));
        assert!(matches!(vals[1], PSValue::Integer(2)));
        assert!(matches!(vals[2], PSValue::Integer(3)));
    }

    #[test]
    fn for_int_step_two() {
        let vals = collect_for_counters(int(0), int(2), int(6));
        assert_eq!(vals.len(), 4);
        assert!(matches!(vals[0], PSValue::Integer(0)));
        assert!(matches!(vals[1], PSValue::Integer(2)));
        assert!(matches!(vals[2], PSValue::Integer(4)));
        assert!(matches!(vals[3], PSValue::Integer(6)));
    }

    #[test]
    fn for_int_descending() {
        let vals = collect_for_counters(int(3), int(-1), int(1));
        assert_eq!(vals.len(), 3);
        assert!(matches!(vals[0], PSValue::Integer(3)));
        assert!(matches!(vals[1], PSValue::Integer(2)));
        assert!(matches!(vals[2], PSValue::Integer(1)));
    }

    #[test]
    fn for_int_initial_equals_limit() {
        // 0 1 0 → one iteration (counter = 0 ≤ limit = 0)
        let vals = collect_for_counters(int(0), int(1), int(0));
        assert_eq!(vals.len(), 1);
        assert!(matches!(vals[0], PSValue::Integer(0)));
    }

    #[test]
    fn for_int_no_iterations_when_initial_exceeds_limit() {
        // 5 1 3 → counter (5) > limit (3) immediately, no body calls
        let vals = collect_for_counters(int(5), int(1), int(3));
        assert_eq!(vals.len(), 0);
    }

    #[test]
    fn for_zero_step_no_iterations() {
        // increment = 0 → body never executes
        let vals = collect_for_counters(int(1), int(0), int(10));
        assert_eq!(vals.len(), 0);
    }

    #[test]
    fn for_float_ascending() {
        // 0.0 0.5 1.5 → counters: 0.0, 0.5, 1.0, 1.5
        let vals = collect_for_counters(flt(0.0), flt(0.5), flt(1.5));
        assert_eq!(vals.len(), 4);
        for v in &vals {
            assert!(matches!(v, PSValue::Float(_)));
        }
        let floats: Vec<f64> = vals.iter().map(|v| {
            if let PSValue::Float(f) = v { *f } else { panic!("expected Float") }
        }).collect();
        assert!((floats[0] - 0.0).abs() < 1e-9);
        assert!((floats[1] - 0.5).abs() < 1e-9);
        assert!((floats[2] - 1.0).abs() < 1e-9);
        assert!((floats[3] - 1.5).abs() < 1e-9);
    }

    #[test]
    fn for_mixed_int_float_promotes_to_float() {
        // One real operand forces the counter to be Float.
        let vals = collect_for_counters(int(0), int(1), flt(2.0));
        assert_eq!(vals.len(), 3);
        for v in &vals {
            assert!(matches!(v, PSValue::Float(_)));
        }
    }

    #[test]
    fn for_proc_receives_counter() {
        // Verify that each iteration the counter is available to the proc.
        // Proc: pop counter, multiply by 2 using inline operator, push result.
        let mul2 = Rc::new(|i: &mut Interpreter| -> Result<(), PSError> {
            match i.pop()? {
                PSValue::Integer(n) => { i.push(PSValue::Integer(n * 2)); Ok(()) }
                _ => Err(PSError::TypeCheck { expected: "integer", got: "other" }),
            }
        });
        // `mul2` pops the counter (which was just pushed by `for`) and pushes counter*2.
        let proc = PSValue::Procedure(Rc::new(vec![PSValue::Operator(mul2)]), None);

        let mut i = Interpreter::new();
        i.push(int(1));
        i.push(int(1));
        i.push(int(3));
        i.push(proc);
        op_for(&mut i).unwrap();

        // Iterations pushed 1*2=2, 2*2=4, 3*2=6.
        assert_eq!(i.operand_stack.len(), 3);
        assert_eq!(pop_int(&mut i), 6);
        assert_eq!(pop_int(&mut i), 4);
        assert_eq!(pop_int(&mut i), 2);
    }

    #[test]
    fn for_non_number_init_typecheck() {
        let mut i = Interpreter::new();
        i.push(bool_(true)); // init — not a number
        i.push(int(1));
        i.push(int(3));
        i.push(empty_proc());
        assert!(matches!(op_for(&mut i), Err(PSError::TypeCheck { .. })));
    }

    #[test]
    fn for_non_proc_typecheck() {
        let mut i = Interpreter::new();
        i.push(int(1));
        i.push(int(1));
        i.push(int(3));
        i.push(int(99)); // not a procedure
        assert!(matches!(op_for(&mut i), Err(PSError::TypeCheck { .. })));
    }

    // ── quit ──────────────────────────────────────────────────────────────────

    #[test]
    fn quit_returns_quit_error() {
        let mut i = Interpreter::new();
        assert!(matches!(op_quit(&mut i), Err(PSError::Quit)));
    }

    #[test]
    fn quit_does_not_consume_stack() {
        // quit should not touch the operand stack at all.
        let mut i = Interpreter::new();
        i.push(int(42));
        i.push(bool_(true));
        let _ = op_quit(&mut i);
        assert_eq!(i.operand_stack.len(), 2);
    }

    // ── composition: if + ifelse are consistent ───────────────────────────────

    #[test]
    fn if_true_equivalent_to_ifelse_true() {
        // `true { 99 } if`  should produce the same result as
        // `true { 99 } { } ifelse`.
        let mut i1 = Interpreter::new();
        i1.push(bool_(true));
        i1.push(push_int_proc(99));
        op_if(&mut i1).unwrap();

        let mut i2 = Interpreter::new();
        i2.push(bool_(true));
        i2.push(push_int_proc(99));
        i2.push(empty_proc());
        op_ifelse(&mut i2).unwrap();

        assert_eq!(pop_int(&mut i1), pop_int(&mut i2));
    }

    #[test]
    fn repeat_one_same_as_exec_proc_directly() {
        // `1 { 55 } repeat` should be equivalent to calling exec_proc once.
        let mut i1 = Interpreter::new();
        i1.push(int(1));
        i1.push(push_int_proc(55));
        op_repeat(&mut i1).unwrap();

        let mut i2 = Interpreter::new();
        i2.exec_proc(push_int_proc(55)).unwrap();

        assert_eq!(pop_int(&mut i1), pop_int(&mut i2));
    }
}
