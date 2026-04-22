use crate::dict_stack::DictStack;
use crate::stack::OperandStack;
use crate::types::{DictRef, PSError, PSValue};
use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

// ── Execution engine ──────────────────────────────────────────────────────────
//
// PostScript separates PUSHING a procedure from RUNNING it.
//
// When the interpreter encounters a `{ ... }` token in the source, it PUSHES
// the `PSValue::Procedure` onto the operand stack as a data value — exactly as
// if it had pushed a number or string.  The procedure body is NOT evaluated yet.
//
// Later, an operator like `if`, `ifelse`, `repeat`, or `for` pops the procedure
// and explicitly asks the interpreter to RUN it by calling `exec_proc`.
//
// `exec_proc` delegates to `exec_body`, which iterates over the body tokens and
// dispatches each one:
//
//   Integer / Float / Boolean / String / Name / Array / Null / Mark
//       → pushed onto the operand stack (data literals).
//
//   Procedure(body, scope)
//       → pushed as data in dynamic mode.
//         In lexical mode the procedure is re-wrapped with the CURRENT dict-stack
//         snapshot via `make_procedure`, so inner lambdas capture the right env.
//
//   Operator(f)
//       → the Rust closure `f` is called immediately.
//         (Operators are normally reached via ExecutableName lookup, but a body
//          built directly in Rust — e.g. in tests — may contain Operator tokens
//          directly.)
//
//   ExecutableName(name)
//       → looked up via `lookup_name` (dynamic or lexical depending on mode).
//         · Procedure result → executed recursively via `exec_body`.
//         · Operator result  → closure called immediately.
//         · Any other value  → pushed onto the operand stack.
//         · Not found        → Err(Undefined).
//
// BORROW SAFETY
//
// `lookup_name` borrows `self` immutably and returns an OWNED `PSValue` (cloned
// from the HashMap).  The immutable borrow ends before any mutable call such as
// `f(self)` or a recursive `exec_body`.  There is therefore no borrow conflict
// even though `exec_body` calls operators that themselves take `&mut self`.

/// Top-level interpreter state.
///
/// All operator functions receive `&mut Interpreter` so they have uniform
/// access to both stacks and the scoping flag.
pub struct Interpreter {
    pub operand_stack: OperandStack,
    pub dict_stack: DictStack,
    /// Destination for `print`, `=`, and `==`.  Defaults to stdout; inject a
    /// `Vec<u8>`-backed writer in tests to capture output without spawning a process.
    pub output: Box<dyn Write>,

    // ── Scoping mode ─────────────────────────────────────────────────────────
    //
    // PostScript originally specifies DYNAMIC scoping: name lookup always walks
    // the CURRENT dict stack at call time, so a procedure sees whatever bindings
    // happen to be on the stack when it is called — not when it was defined.
    //
    // Setting `use_lexical_scope = true` switches to LEXICAL scoping: each
    // procedure carries a snapshot of the dict stack taken at definition time,
    // and lookup searches that snapshot instead of the live stack.
    //
    // The flag affects two things:
    //   1. `make_procedure`  — whether to attach a captured scope to the value.
    //   2. `lookup_name`     — which scope chain to search (live vs captured).
    //
    // The execution loop (implemented separately) uses both of these when it
    // encounters an executable name or a procedure literal in the program.
    pub use_lexical_scope: bool,
}

impl Interpreter {
    pub fn new() -> Self {
        Self::with_output(Box::new(std::io::stdout()))
    }

    /// Create an interpreter that writes to a custom sink.
    /// Pass a `Box<dyn Write>` — typically a `Vec<u8>` wrapper in tests.
    pub fn with_output(output: Box<dyn Write>) -> Self {
        Self {
            operand_stack: OperandStack::default(),
            dict_stack: DictStack::new(),
            use_lexical_scope: false,
            output,
        }
    }

    // ── Operand stack helpers ─────────────────────────────────────────────────

    pub fn push(&mut self, val: PSValue) {
        self.operand_stack.push(val);
    }

    pub fn pop(&mut self) -> Result<PSValue, PSError> {
        self.operand_stack.pop()
    }

    pub fn peek(&self) -> Result<&PSValue, PSError> {
        self.operand_stack.peek()
    }

    // ── Name lookup ───────────────────────────────────────────────────────────

    /// Look up `key` using the CURRENT live dict stack (dynamic scoping).
    ///
    /// This is the lookup path used when `use_lexical_scope = false`, or when
    /// no captured scope is available for the executing procedure.
    pub fn lookup_dynamic(&self, key: &str) -> Option<PSValue> {
        self.dict_stack.lookup(key)
    }

    /// Look up `key` in a captured scope snapshot (lexical scoping).
    ///
    /// `scope` is the `Vec<DictRef>` stored inside a `PSValue::Procedure` that
    /// was created while `use_lexical_scope = true`.  The execution loop passes
    /// the executing procedure's captured scope here.
    pub fn lookup_lexical(scope: &[DictRef], key: &str) -> Option<PSValue> {
        DictStack::lookup_in(scope, key)
    }

    /// Primary name-lookup entry point used by the execution loop.
    ///
    /// • `exec_scope` is the captured scope of the currently-executing
    ///   procedure, or `None` if we are at the top level.
    ///
    /// Behaviour:
    ///   use_lexical_scope = false  →  always use the live dict stack.
    ///   use_lexical_scope = true   →  if a captured scope is provided, search
    ///                                 that; fall back to the live stack only if
    ///                                 the name is not found there (so built-ins
    ///                                 registered in systemdict remain visible).
    pub fn lookup_name(&self, key: &str, exec_scope: Option<&[DictRef]>) -> Option<PSValue> {
        if self.use_lexical_scope {
            if let Some(scope) = exec_scope {
                // Search the captured scope first.  If not found, fall back to
                // the live stack so that systemdict operators are always reachable.
                return Self::lookup_lexical(scope, key)
                    .or_else(|| self.dict_stack.lookup(key));
            }
        }
        // Dynamic mode, or top-level with no captured scope.
        self.dict_stack.lookup(key)
    }

    // ── Procedure construction ────────────────────────────────────────────────

    // ── Execution ─────────────────────────────────────────────────────────────

    /// Run a `PSValue::Procedure` (or `Operator`) that was previously popped from
    /// the operand stack.
    ///
    /// Called by `if`, `ifelse`, `repeat`, `for`, and `exec` after they have
    /// obtained a procedure value.  Any other `PSValue` is a `typecheck`.
    pub fn exec_proc(&mut self, proc: PSValue) -> Result<(), PSError> {
        match proc {
            PSValue::Procedure(body, captured) => {
                let body_rc = Rc::clone(&body);
                match captured {
                    None => self.exec_body(&body_rc, None),
                    Some(scope_rc) => self.exec_body(&body_rc, Some(&scope_rc)),
                }
            }
            PSValue::Operator(f) => f(self),
            other => {
                self.push(other);
                Err(PSError::TypeCheck { expected: "procedure", got: "non-procedure" })
            }
        }
    }

    /// Evaluate a slice of `PSValue` tokens — the inner execution engine.
    ///
    /// `exec_scope` is the captured dict-stack snapshot of the procedure that
    /// contains this body (for lexical scoping), or `None` for dynamic mode or
    /// top-level execution.
    pub fn exec_body(
        &mut self,
        body: &[PSValue],
        exec_scope: Option<&[DictRef]>,
    ) -> Result<(), PSError> {
        for token in body {
            match token {
                PSValue::ExecutableName(name) => {
                    // Returns an owned PSValue — immutable borrow of self ends here.
                    let found = self.lookup_name(name, exec_scope);
                    match found {
                        None => return Err(PSError::Undefined(name.to_string())),
                        Some(PSValue::Operator(f)) => f(self)?,
                        Some(PSValue::Procedure(proc_body, captured)) => {
                            let body2 = Rc::clone(&proc_body);
                            match captured {
                                None => self.exec_body(&body2, None)?,
                                Some(scope_rc) => {
                                    self.exec_body(&body2, Some(&scope_rc))?;
                                }
                            }
                        }
                        Some(other) => self.push(other),
                    }
                }
                PSValue::Operator(f) => {
                    // Operator token embedded directly in a body (common in tests).
                    let f = Rc::clone(f);
                    f(self)?;
                }
                PSValue::Procedure(proc_body, _) if self.use_lexical_scope => {
                    // Procedure literal in lexical mode: re-capture current scope
                    // so inner lambdas see the right environment.
                    let body2 = Rc::clone(proc_body);
                    let proc = self.make_procedure(body2);
                    self.push(proc);
                }
                other => self.push(other.clone()),
            }
        }
        Ok(())
    }

    /// Wrap a parsed procedure body into a `PSValue::Procedure`.
    ///
    /// When `use_lexical_scope = true` this also captures a snapshot of the
    /// current dict stack and stores it inside the value.  The snapshot is taken
    /// HERE (at the moment the procedure literal is evaluated / pushed) — not
    /// later when the procedure is called.
    ///
    /// When `use_lexical_scope = false` the captured scope is `None` and no
    /// snapshot overhead is incurred.
    pub fn make_procedure(&self, body: Rc<Vec<PSValue>>) -> PSValue {
        let captured = if self.use_lexical_scope {
            Some(Rc::new(self.dict_stack.snapshot()))
        } else {
            None
        };
        PSValue::Procedure(body, captured)
    }

    /// Register all built-in operators into `systemdict`.
    ///
    /// Must be called once after `new()` before executing any parsed PostScript
    /// source.  Tests that build programs directly from `PSValue` tokens and call
    /// operator functions by name do not need this; it is only required when the
    /// execution engine resolves names through the dict stack (i.e. in the REPL
    /// and any full-program runner).
    pub fn register_builtins(&mut self) {
        use crate::ops::arithmetic::*;
        use crate::ops::comparison::*;
        use crate::ops::control::*;
        use crate::ops::dict::*;
        use crate::ops::io::*;
        use crate::ops::stack::*;
        use crate::ops::string::*;

        macro_rules! reg {
            ($name:literal, $fn:expr) => {
                self.dict_stack.systemdict().borrow_mut().insert(
                    $name.to_string(),
                    PSValue::Operator(Rc::new($fn)),
                );
            };
        }

        // Arithmetic
        reg!("add",     op_add);
        reg!("sub",     op_sub);
        reg!("mul",     op_mul);
        reg!("div",     op_div);
        reg!("idiv",    op_idiv);
        reg!("mod",     op_mod);
        reg!("abs",     op_abs);
        reg!("neg",     op_neg);
        reg!("ceiling", op_ceiling);
        reg!("floor",   op_floor);
        reg!("round",   op_round);
        reg!("sqrt",    op_sqrt);

        // Comparison
        reg!("eq", op_eq);
        reg!("ne", op_ne);
        reg!("lt", op_lt);
        reg!("le", op_le);
        reg!("gt", op_gt);
        reg!("ge", op_ge);

        // Logical / bitwise
        reg!("and",   op_and);
        reg!("or",    op_or);
        reg!("not",   op_not);
        reg!("true",  op_true);
        reg!("false", op_false);

        // Stack
        reg!("dup",   op_dup);
        reg!("exch",  op_exch);
        reg!("pop",   op_pop);
        reg!("copy",  op_copy);
        reg!("clear", op_clear);
        reg!("count", op_count);

        // Dictionary
        reg!("dict",      op_dict);
        reg!("maxlength", op_maxlength);
        reg!("begin",     op_begin);
        reg!("end",       op_end);
        reg!("def",       op_def);

        // String
        reg!("get",          op_get);
        reg!("getinterval",  op_getinterval);
        reg!("putinterval",  op_putinterval);

        // Control flow
        reg!("if",     op_if);
        reg!("ifelse", op_ifelse);
        reg!("repeat", op_repeat);
        reg!("for",    op_for);
        reg!("quit",   op_quit);

        // I/O
        reg!("print", op_print);
        reg!("=",     op_equal);
        reg!("==",    op_equal_equal);

        // Polymorphic `length` — dispatches on the type of the top-of-stack value.
        // Both ops::string and ops::dict define their own `op_length`; this unified
        // entry point checks the type and calls the right one.
        reg!("length", |interp: &mut Interpreter| {
            match interp.peek()? {
                PSValue::String(_)     => crate::ops::string::op_length(interp),
                PSValue::Dictionary(_) => crate::ops::dict::op_length(interp),
                other => {
                    let got = match other {
                        PSValue::Integer(_) => "integer",
                        PSValue::Float(_)   => "real",
                        PSValue::Boolean(_) => "boolean",
                        PSValue::Array(_)   => "array",
                        _                  => "unknown",
                    };
                    Err(PSError::TypeCheck { expected: "string or dict", got })
                }
            }
        });

        // `]` — collect everything above the nearest mark into an Array.
        reg!("]", |interp: &mut Interpreter| {
            let mut items: Vec<PSValue> = Vec::new();
            loop {
                match interp.pop()? {
                    PSValue::Mark => break,
                    val => items.push(val),
                }
            }
            items.reverse();
            interp.push(PSValue::Array(Rc::new(RefCell::new(items))));
            Ok(())
        });

        // `mark` — push a mark object.
        reg!("mark", |interp: &mut Interpreter| {
            interp.push(PSValue::Mark);
            Ok(())
        });

        // `pstack` — print every value on the stack (top first) without consuming them.
        reg!("pstack", |interp: &mut Interpreter| {
            let items = interp.operand_stack.as_slice().to_vec();
            for val in items.iter().rev() {
                let line = format!("{}\n", crate::ops::io::ps_repr(val));
                interp.output.write_all(line.as_bytes())
                    .map_err(|e| PSError::Other(e.to_string()))?;
            }
            Ok(())
        });
    }
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}
