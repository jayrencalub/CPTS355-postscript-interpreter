// ── Comparison and logical operators ─────────────────────────────────────────
//
// This module implements two related groups of operators:
//
//   RELATIONAL  — eq, ne, lt, le, gt, ge
//   LOGICAL     — and, or, not, true, false
//
// ── Type rules ────────────────────────────────────────────────────────────────
//
// RELATIONAL OPERATORS
//
//   eq / ne   accept ANY two operands.  For numbers they promote int→float when
//             the types differ.  For compound objects (Array, Dictionary,
//             Procedure) equality means REFERENCE IDENTITY (same Rc pointer),
//             not structural equality — two separately-created arrays with
//             identical contents are NOT equal.  This matches PostScript
//             semantics.
//
//   lt / le / gt / ge   accept only numbers (int or float, with promotion)
//             or strings (lexicographic byte comparison).  Mixing a number
//             with a string is a typecheck error.
//
// LOGICAL OPERATORS
//
//   and / or  are OVERLOADED across two distinct types:
//
//       Boolean × Boolean → Boolean    logical AND / OR
//       Integer × Integer → Integer    bitwise AND / OR
//
//     Mixing booleans with integers is a typecheck error.  This is a
//     deliberate PostScript design choice: integers serve double duty as
//     bit-vectors, so the same operator token does two different things
//     depending on the operand types.
//
//   not   is similarly overloaded:
//
//       Boolean → Boolean    logical NOT
//       Integer → Integer    bitwise NOT (Rust's `!` operator: ~n in C terms)
//
//     Note: bitwise NOT of a signed i64 follows two's-complement rules.
//     `not 5` → `!5_i64` → `-6`.
//
// TRUE / FALSE
//
//   In PostScript, `true` and `false` are executable names that look up to
//   boolean literals.  These operator functions push the appropriate boolean
//   directly, bypassing the dict-stack lookup.  They are registered in
//   systemdict by the interpreter at startup.

use std::cmp::Ordering;
use std::rc::Rc;

use crate::interpreter::Interpreter;
use crate::types::{PSError, PSValue};

// ── Equality helpers ──────────────────────────────────────────────────────────

/// Structural/reference equality following PostScript semantics.
///
/// Rules:
///   • Numbers    — numeric comparison with int↔float promotion.
///   • Booleans   — value equality.
///   • Strings    — byte-for-byte comparison of the VIEW contents.
///   • Names      — string comparison; `Name` and `ExecutableName` share the
///                  same namespace, so `/foo eq foo` is true.
///   • Arrays / Dictionaries / Procedures — REFERENCE IDENTITY (`Rc::ptr_eq`).
///   • Null       — always equal to Null.
///   • Different non-numeric types — false.
fn ps_eq(a: &PSValue, b: &PSValue) -> bool {
    match (a, b) {
        // ── Numeric (promote int→float when mixed) ──
        (PSValue::Integer(x), PSValue::Integer(y)) => x == y,
        (PSValue::Float(x),   PSValue::Float(y))   => x == y,
        (PSValue::Integer(x), PSValue::Float(y))   => (*x as f64) == *y,
        (PSValue::Float(x),   PSValue::Integer(y)) => *x == (*y as f64),

        // ── Boolean ──
        (PSValue::Boolean(x), PSValue::Boolean(y)) => x == y,

        // ── String: compare the bytes of each view ──
        (PSValue::String(x), PSValue::String(y)) => x.to_bytes() == y.to_bytes(),

        // ── Names: same namespace regardless of variant ──
        (PSValue::Name(x),           PSValue::Name(y))           => x == y,
        (PSValue::Name(x),           PSValue::ExecutableName(y)) => x == y,
        (PSValue::ExecutableName(x), PSValue::Name(y))           => x == y,
        (PSValue::ExecutableName(x), PSValue::ExecutableName(y)) => x == y,

        // ── Compound objects: reference identity ──
        (PSValue::Array(x),      PSValue::Array(y))      => Rc::ptr_eq(x, y),
        (PSValue::Dictionary(x), PSValue::Dictionary(y)) => Rc::ptr_eq(x, y),
        // Compare procedure bodies; ignore the captured scope.
        (PSValue::Procedure(x, _), PSValue::Procedure(y, _)) => Rc::ptr_eq(x, y),

        // ── Null ──
        (PSValue::Null, PSValue::Null) => true,

        // ── Anything else (mismatched types) ──
        _ => false,
    }
}

// ── Ordering helpers ──────────────────────────────────────────────────────────

/// An operand for ordered comparison: either a number (always widened to f64)
/// or a byte string (compared lexicographically).
enum OrdVal {
    Num(f64),
    Str(Vec<u8>),
}

/// Convert a `PSValue` into an `OrdVal`.
/// Only integers, floats, and strings are valid; anything else is a typecheck.
fn to_ord(v: PSValue) -> Result<OrdVal, PSError> {
    match v {
        PSValue::Integer(n) => Ok(OrdVal::Num(n as f64)),
        PSValue::Float(f)   => Ok(OrdVal::Num(f)),
        PSValue::String(s)  => Ok(OrdVal::Str(s.to_bytes())),
        _ => Err(PSError::TypeCheck {
            expected: "number or string",
            got:      "non-comparable type",
        }),
    }
}

/// Compare two `OrdVal`s.
///
/// Mixed number-vs-string is a typecheck.  NaN comparisons produce
/// `Err(Other("undefined"))` because no ordering can be defined for NaN.
fn ord_cmp(a: OrdVal, b: OrdVal) -> Result<Ordering, PSError> {
    match (a, b) {
        (OrdVal::Num(x), OrdVal::Num(y)) => x.partial_cmp(&y)
            .ok_or_else(|| PSError::Other("undefined: NaN comparison".into())),
        (OrdVal::Str(x), OrdVal::Str(y)) => Ok(x.cmp(&y)),
        _ => Err(PSError::TypeCheck {
            expected: "matching types (num-num or string-string)",
            got:      "mixed number and string",
        }),
    }
}

// ── Relational operators ──────────────────────────────────────────────────────

/// `eq` — test equality.
///
/// Stack effect: `any1 any2 → bool`
///
/// Works on any pair of operands.  See `ps_eq` for the full type rules.
pub fn op_eq(interp: &mut Interpreter) -> Result<(), PSError> {
    let b = interp.pop()?;
    let a = interp.pop()?;
    interp.push(PSValue::Boolean(ps_eq(&a, &b)));
    Ok(())
}

/// `ne` — test inequality.
///
/// Stack effect: `any1 any2 → bool`
///
/// Equivalent to `eq not`.
pub fn op_ne(interp: &mut Interpreter) -> Result<(), PSError> {
    let b = interp.pop()?;
    let a = interp.pop()?;
    interp.push(PSValue::Boolean(!ps_eq(&a, &b)));
    Ok(())
}

/// `lt` — less than.
///
/// Stack effect: `num1|str1 num2|str2 → bool`
///
/// Errors: `typecheck` if operands are not both numbers or both strings.
pub fn op_lt(interp: &mut Interpreter) -> Result<(), PSError> {
    let b = to_ord(interp.pop()?)?;
    let a = to_ord(interp.pop()?)?;
    interp.push(PSValue::Boolean(ord_cmp(a, b)? == Ordering::Less));
    Ok(())
}

/// `le` — less than or equal.
///
/// Stack effect: `num1|str1 num2|str2 → bool`
pub fn op_le(interp: &mut Interpreter) -> Result<(), PSError> {
    let b = to_ord(interp.pop()?)?;
    let a = to_ord(interp.pop()?)?;
    interp.push(PSValue::Boolean(ord_cmp(a, b)? != Ordering::Greater));
    Ok(())
}

/// `gt` — greater than.
///
/// Stack effect: `num1|str1 num2|str2 → bool`
pub fn op_gt(interp: &mut Interpreter) -> Result<(), PSError> {
    let b = to_ord(interp.pop()?)?;
    let a = to_ord(interp.pop()?)?;
    interp.push(PSValue::Boolean(ord_cmp(a, b)? == Ordering::Greater));
    Ok(())
}

/// `ge` — greater than or equal.
///
/// Stack effect: `num1|str1 num2|str2 → bool`
pub fn op_ge(interp: &mut Interpreter) -> Result<(), PSError> {
    let b = to_ord(interp.pop()?)?;
    let a = to_ord(interp.pop()?)?;
    interp.push(PSValue::Boolean(ord_cmp(a, b)? != Ordering::Less));
    Ok(())
}

// ── Logical operators ─────────────────────────────────────────────────────────

/// `and` — logical AND (booleans) or bitwise AND (integers).
///
/// Stack effect:
///   `bool1 bool2 → bool`    logical AND
///   `int1  int2  → int`     bitwise AND
///
/// Errors: `typecheck` if the two operands are not both boolean or both integer.
/// Mixed operands (one bool, one int) are not allowed.
pub fn op_and(interp: &mut Interpreter) -> Result<(), PSError> {
    let b = interp.pop()?;
    let a = interp.pop()?;
    let result = match (a, b) {
        (PSValue::Boolean(x), PSValue::Boolean(y)) => PSValue::Boolean(x && y),
        (PSValue::Integer(x), PSValue::Integer(y)) => PSValue::Integer(x & y),
        _ => return Err(PSError::TypeCheck {
            expected: "bool-bool or int-int",
            got:      "mismatched or unsupported types",
        }),
    };
    interp.push(result);
    Ok(())
}

/// `or` — logical OR (booleans) or bitwise OR (integers).
///
/// Stack effect:
///   `bool1 bool2 → bool`    logical OR
///   `int1  int2  → int`     bitwise OR
///
/// Errors: `typecheck` if operands are not both boolean or both integer.
pub fn op_or(interp: &mut Interpreter) -> Result<(), PSError> {
    let b = interp.pop()?;
    let a = interp.pop()?;
    let result = match (a, b) {
        (PSValue::Boolean(x), PSValue::Boolean(y)) => PSValue::Boolean(x || y),
        (PSValue::Integer(x), PSValue::Integer(y)) => PSValue::Integer(x | y),
        _ => return Err(PSError::TypeCheck {
            expected: "bool-bool or int-int",
            got:      "mismatched or unsupported types",
        }),
    };
    interp.push(result);
    Ok(())
}

/// `not` — logical NOT (boolean) or bitwise complement (integer).
///
/// Stack effect:
///   `bool → bool`    logical NOT
///   `int  → int`     bitwise NOT (two's-complement; `not 5` → `-6`)
///
/// Errors: `typecheck` for any other input type; value is pushed back.
pub fn op_not(interp: &mut Interpreter) -> Result<(), PSError> {
    let a = interp.pop()?;
    let result = match a {
        PSValue::Boolean(x) => PSValue::Boolean(!x),
        // Rust's `!` on i64 is bitwise complement — matches PostScript.
        PSValue::Integer(x) => PSValue::Integer(!x),
        other => {
            interp.push(other); // restore so the stack is not silently consumed
            return Err(PSError::TypeCheck {
                expected: "bool or integer",
                got:      "other",
            });
        }
    };
    interp.push(result);
    Ok(())
}

/// `true` — push the boolean value `true`.
///
/// Stack effect: `→ true`
///
/// In PostScript `true` is an executable name resolved via systemdict.
/// This function is the operator registered under that name.
pub fn op_true(interp: &mut Interpreter) -> Result<(), PSError> {
    interp.push(PSValue::Boolean(true));
    Ok(())
}

/// `false` — push the boolean value `false`.
///
/// Stack effect: `→ false`
pub fn op_false(interp: &mut Interpreter) -> Result<(), PSError> {
    interp.push(PSValue::Boolean(false));
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PSString;
    use std::cell::RefCell;

    fn int(n: i64)    -> PSValue { PSValue::Integer(n) }
    fn flt(f: f64)    -> PSValue { PSValue::Float(f) }
    fn bool_(b: bool) -> PSValue { PSValue::Boolean(b) }
    fn str_(s: &[u8]) -> PSValue { PSValue::String(PSString::new(s.to_vec())) }
    fn name(s: &str)  -> PSValue { PSValue::Name(Rc::from(s)) }
    fn xname(s: &str) -> PSValue { PSValue::ExecutableName(Rc::from(s)) }

    fn pop_bool(i: &mut Interpreter) -> bool {
        match i.pop().unwrap() {
            PSValue::Boolean(b) => b,
            other => panic!("expected Boolean, got: {other}"),
        }
    }
    fn pop_int(i: &mut Interpreter) -> i64 {
        match i.pop().unwrap() {
            PSValue::Integer(n) => n,
            other => panic!("expected Integer, got: {other}"),
        }
    }

    // ── eq ────────────────────────────────────────────────────────────────────

    #[test]
    fn eq_int_int_equal() {
        let mut i = Interpreter::new();
        i.push(int(5)); i.push(int(5));
        op_eq(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn eq_int_int_unequal() {
        let mut i = Interpreter::new();
        i.push(int(3)); i.push(int(4));
        op_eq(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    #[test]
    fn eq_int_float_equal() {
        // PostScript: `1 1.0 eq` → true
        let mut i = Interpreter::new();
        i.push(int(1)); i.push(flt(1.0));
        op_eq(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn eq_int_float_unequal() {
        let mut i = Interpreter::new();
        i.push(int(1)); i.push(flt(1.5));
        op_eq(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    #[test]
    fn eq_bool_equal() {
        let mut i = Interpreter::new();
        i.push(bool_(true)); i.push(bool_(true));
        op_eq(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn eq_bool_unequal() {
        let mut i = Interpreter::new();
        i.push(bool_(true)); i.push(bool_(false));
        op_eq(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    #[test]
    fn eq_strings_equal() {
        let mut i = Interpreter::new();
        i.push(str_(b"hello")); i.push(str_(b"hello"));
        op_eq(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn eq_strings_unequal() {
        let mut i = Interpreter::new();
        i.push(str_(b"hello")); i.push(str_(b"world"));
        op_eq(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    #[test]
    fn eq_name_and_executable_name_with_same_string() {
        // /foo eq foo  →  true  (same name namespace)
        let mut i = Interpreter::new();
        i.push(name("foo")); i.push(xname("foo"));
        op_eq(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn eq_different_types_false() {
        // integer vs boolean — never equal
        let mut i = Interpreter::new();
        i.push(int(1)); i.push(bool_(true));
        op_eq(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    #[test]
    fn eq_arrays_same_rc_true() {
        let mut i = Interpreter::new();
        let arr = PSValue::Array(Rc::new(RefCell::new(vec![])));
        i.push(arr.clone()); i.push(arr);
        op_eq(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn eq_arrays_different_rc_false() {
        let mut i = Interpreter::new();
        i.push(PSValue::Array(Rc::new(RefCell::new(vec![]))));
        i.push(PSValue::Array(Rc::new(RefCell::new(vec![]))));
        op_eq(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    // ── ne ────────────────────────────────────────────────────────────────────

    #[test]
    fn ne_equal_values_false() {
        let mut i = Interpreter::new();
        i.push(int(7)); i.push(int(7));
        op_ne(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    #[test]
    fn ne_unequal_values_true() {
        let mut i = Interpreter::new();
        i.push(int(7)); i.push(int(8));
        op_ne(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    // ── lt ────────────────────────────────────────────────────────────────────

    #[test]
    fn lt_int_int_true() {
        let mut i = Interpreter::new();
        i.push(int(3)); i.push(int(5));
        op_lt(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn lt_int_int_equal_false() {
        let mut i = Interpreter::new();
        i.push(int(5)); i.push(int(5));
        op_lt(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    #[test]
    fn lt_int_float_promotion() {
        let mut i = Interpreter::new();
        i.push(int(2)); i.push(flt(2.5));
        op_lt(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn lt_strings_lexicographic() {
        let mut i = Interpreter::new();
        i.push(str_(b"abc")); i.push(str_(b"abd"));
        op_lt(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn lt_string_vs_number_typecheck() {
        let mut i = Interpreter::new();
        i.push(str_(b"abc")); i.push(int(1));
        assert!(matches!(op_lt(&mut i), Err(PSError::TypeCheck { .. })));
    }

    // ── le ────────────────────────────────────────────────────────────────────

    #[test]
    fn le_equal_true() {
        let mut i = Interpreter::new();
        i.push(int(4)); i.push(int(4));
        op_le(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn le_less_true() {
        let mut i = Interpreter::new();
        i.push(int(3)); i.push(int(4));
        op_le(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn le_greater_false() {
        let mut i = Interpreter::new();
        i.push(int(5)); i.push(int(4));
        op_le(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    // ── gt ────────────────────────────────────────────────────────────────────

    #[test]
    fn gt_true() {
        let mut i = Interpreter::new();
        i.push(int(9)); i.push(int(3));
        op_gt(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn gt_equal_false() {
        let mut i = Interpreter::new();
        i.push(int(4)); i.push(int(4));
        op_gt(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    // ── ge ────────────────────────────────────────────────────────────────────

    #[test]
    fn ge_equal_true() {
        let mut i = Interpreter::new();
        i.push(int(4)); i.push(int(4));
        op_ge(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn ge_greater_true() {
        let mut i = Interpreter::new();
        i.push(int(5)); i.push(int(4));
        op_ge(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn ge_less_false() {
        let mut i = Interpreter::new();
        i.push(int(3)); i.push(int(4));
        op_ge(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    // ── and ───────────────────────────────────────────────────────────────────

    #[test]
    fn and_bool_true_true() {
        let mut i = Interpreter::new();
        i.push(bool_(true)); i.push(bool_(true));
        op_and(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn and_bool_true_false() {
        let mut i = Interpreter::new();
        i.push(bool_(true)); i.push(bool_(false));
        op_and(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    #[test]
    fn and_bool_false_false() {
        let mut i = Interpreter::new();
        i.push(bool_(false)); i.push(bool_(false));
        op_and(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    #[test]
    fn and_int_bitwise() {
        // 0b1010 & 0b1100 = 0b1000
        let mut i = Interpreter::new();
        i.push(int(0b1010)); i.push(int(0b1100));
        op_and(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 0b1000);
    }

    #[test]
    fn and_int_all_bits() {
        // -1 is all 1-bits; -1 & n = n
        let mut i = Interpreter::new();
        i.push(int(-1)); i.push(int(42));
        op_and(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 42);
    }

    #[test]
    fn and_mixed_bool_int_typecheck() {
        let mut i = Interpreter::new();
        i.push(bool_(true)); i.push(int(1));
        assert!(matches!(op_and(&mut i), Err(PSError::TypeCheck { .. })));
    }

    // ── or ────────────────────────────────────────────────────────────────────

    #[test]
    fn or_bool_false_false() {
        let mut i = Interpreter::new();
        i.push(bool_(false)); i.push(bool_(false));
        op_or(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    #[test]
    fn or_bool_false_true() {
        let mut i = Interpreter::new();
        i.push(bool_(false)); i.push(bool_(true));
        op_or(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn or_int_bitwise() {
        // 0b1010 | 0b0101 = 0b1111
        let mut i = Interpreter::new();
        i.push(int(0b1010)); i.push(int(0b0101));
        op_or(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 0b1111);
    }

    #[test]
    fn or_mixed_typecheck() {
        let mut i = Interpreter::new();
        i.push(int(0)); i.push(bool_(false));
        assert!(matches!(op_or(&mut i), Err(PSError::TypeCheck { .. })));
    }

    // ── not ───────────────────────────────────────────────────────────────────

    #[test]
    fn not_bool_true_to_false() {
        let mut i = Interpreter::new();
        i.push(bool_(true));
        op_not(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    #[test]
    fn not_bool_false_to_true() {
        let mut i = Interpreter::new();
        i.push(bool_(false));
        op_not(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn not_int_bitwise_complement() {
        // !5_i64 = -6  (two's complement: ~0b...0101 = 0b...1010 = -6)
        let mut i = Interpreter::new();
        i.push(int(5));
        op_not(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), -6);
    }

    #[test]
    fn not_int_zero_gives_minus_one() {
        // !0 = -1 (all bits set)
        let mut i = Interpreter::new();
        i.push(int(0));
        op_not(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), -1);
    }

    #[test]
    fn not_int_minus_one_gives_zero() {
        // !(-1) = 0
        let mut i = Interpreter::new();
        i.push(int(-1));
        op_not(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 0);
    }

    #[test]
    fn not_typecheck_restores_stack() {
        // non-bool non-int → typecheck; the value is pushed back
        let mut i = Interpreter::new();
        i.push(str_(b"bad"));
        assert!(matches!(op_not(&mut i), Err(PSError::TypeCheck { .. })));
        // Stack should still have the string
        assert_eq!(i.operand_stack.len(), 1);
    }

    // ── true / false ──────────────────────────────────────────────────────────

    #[test]
    fn op_true_pushes_true() {
        let mut i = Interpreter::new();
        op_true(&mut i).unwrap();
        assert!(pop_bool(&mut i));
    }

    #[test]
    fn op_false_pushes_false() {
        let mut i = Interpreter::new();
        op_false(&mut i).unwrap();
        assert!(!pop_bool(&mut i));
    }

    // ── composition ───────────────────────────────────────────────────────────

    #[test]
    fn not_of_eq_same_as_ne() {
        // eq not  ≡  ne
        let mut i1 = Interpreter::new();
        i1.push(int(3)); i1.push(int(4));
        op_eq(&mut i1).unwrap();
        op_not(&mut i1).unwrap();

        let mut i2 = Interpreter::new();
        i2.push(int(3)); i2.push(int(4));
        op_ne(&mut i2).unwrap();

        assert_eq!(pop_bool(&mut i1), pop_bool(&mut i2));
    }

    #[test]
    fn de_morgan_and_or_not() {
        // De Morgan: (A and B) not  ≡  (A not) (B not) or
        // Test with A=true, B=false
        let mut i1 = Interpreter::new();
        i1.push(bool_(true)); i1.push(bool_(false));
        op_and(&mut i1).unwrap();
        op_not(&mut i1).unwrap();

        let mut i2 = Interpreter::new();
        i2.push(bool_(true));  op_not(&mut i2).unwrap();
        i2.push(bool_(false)); op_not(&mut i2).unwrap();
        op_or(&mut i2).unwrap();

        assert_eq!(pop_bool(&mut i1), pop_bool(&mut i2));
    }
}
