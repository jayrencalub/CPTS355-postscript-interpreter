use crate::interpreter::Interpreter;
use crate::types::{PSError, PSValue};

// ── Numeric coercion helpers ──────────────────────────────────────────────────

/// A numeric value that is either an integer or a real, mirroring PostScript's
/// two numeric types.  Used internally to avoid repeating match arms.
enum Num {
    Int(i64),
    Float(f64),
}

impl Num {
    fn from_val(v: PSValue) -> Result<Self, PSError> {
        match v {
            PSValue::Integer(n) => Ok(Num::Int(n)),
            PSValue::Float(f)   => Ok(Num::Float(f)),
            other => Err(PSError::TypeCheck {
                expected: "number",
                got: ps_type_name(&other),
            }),
        }
    }

    fn as_float(&self) -> f64 {
        match self {
            Num::Int(n) => *n as f64,
            Num::Float(f) => *f,
        }
    }

    fn into_psvalue(self) -> PSValue {
        match self {
            Num::Int(n) => PSValue::Integer(n),
            Num::Float(f) => PSValue::Float(f),
        }
    }
}

/// Return the PostScript type name of a value — used in TypeCheck messages.
fn ps_type_name(v: &PSValue) -> &'static str {
    match v {
        PSValue::Integer(_)      => "integer",
        PSValue::Float(_)        => "real",
        PSValue::Boolean(_)      => "boolean",
        PSValue::String(_)       => "string",
        PSValue::Name(_)
        | PSValue::ExecutableName(_) => "name",
        PSValue::Array(_)        => "array",
        PSValue::Dictionary(_)   => "dict",
        PSValue::Procedure(..)   => "procedure",
        PSValue::Null            => "null",
        PSValue::Operator(_)     => "operator",
        PSValue::Mark            => "mark",
    }
}

fn pop_num(interp: &mut Interpreter) -> Result<Num, PSError> {
    Num::from_val(interp.pop()?)
}

/// Pop two numbers (b on top, a below) and return them as `(a, b)`.
fn pop_two(interp: &mut Interpreter) -> Result<(Num, Num), PSError> {
    let b = pop_num(interp)?;
    let a = pop_num(interp)?;
    Ok((a, b))
}

/// Apply a binary arithmetic operation with PostScript's numeric promotion rule:
/// integer op integer → integer; any float operand → float result.
fn apply_binary<FI, FF>(a: Num, b: Num, fi: FI, ff: FF) -> PSValue
where
    FI: Fn(i64, i64) -> i64,
    FF: Fn(f64, f64) -> f64,
{
    match (a, b) {
        (Num::Int(ai), Num::Int(bi)) => PSValue::Integer(fi(ai, bi)),
        (a, b) => PSValue::Float(ff(a.as_float(), b.as_float())),
    }
}

// ── Binary operators ──────────────────────────────────────────────────────────

/// `add` — add two numbers.
///
/// Stack effect: `num1 num2 → sum`
/// Type rule: int+int → int; any float → float.
pub fn op_add(interp: &mut Interpreter) -> Result<(), PSError> {
    let (a, b) = pop_two(interp)?;
    interp.push(apply_binary(a, b, |x, y| x.wrapping_add(y), |x, y| x + y));
    Ok(())
}

/// `sub` — subtract the top from the second.
///
/// Stack effect: `num1 num2 → difference`
pub fn op_sub(interp: &mut Interpreter) -> Result<(), PSError> {
    let (a, b) = pop_two(interp)?;
    interp.push(apply_binary(a, b, |x, y| x.wrapping_sub(y), |x, y| x - y));
    Ok(())
}

/// `mul` — multiply two numbers.
///
/// Stack effect: `num1 num2 → product`
pub fn op_mul(interp: &mut Interpreter) -> Result<(), PSError> {
    let (a, b) = pop_two(interp)?;
    interp.push(apply_binary(a, b, |x, y| x.wrapping_mul(y), |x, y| x * y));
    Ok(())
}

/// `div` — real division; result is always a float.
///
/// Stack effect: `num1 num2 → quotient`
/// Errors: `undefinedresult` if num2 is zero.
pub fn op_div(interp: &mut Interpreter) -> Result<(), PSError> {
    let (a, b) = pop_two(interp)?;
    let bf = b.as_float();
    if bf == 0.0 { return Err(PSError::UndefinedResult); }
    interp.push(PSValue::Float(a.as_float() / bf));
    Ok(())
}

/// `idiv` — integer (truncating) division.
///
/// Stack effect: `int1 int2 → quotient`
/// Both operands must be integers. Truncates toward zero.
/// Errors: `typecheck` if either operand is not an integer.
///         `undefinedresult` if int2 is zero.
pub fn op_idiv(interp: &mut Interpreter) -> Result<(), PSError> {
    let b = match interp.pop()? {
        PSValue::Integer(n) => n,
        other => {
            interp.push(other);
            return Err(PSError::TypeCheck { expected: "integer", got: "non-integer" });
        }
    };
    let a = match interp.pop()? {
        PSValue::Integer(n) => n,
        other => {
            interp.push(other);
            return Err(PSError::TypeCheck { expected: "integer", got: "non-integer" });
        }
    };
    if b == 0 { return Err(PSError::UndefinedResult); }
    interp.push(PSValue::Integer(a / b)); // Rust `/` truncates toward zero — matches PS
    Ok(())
}

/// `mod` — integer remainder with the sign of the dividend.
///
/// Stack effect: `int1 int2 → remainder`
/// Both operands must be integers.
/// Errors: `typecheck` if either operand is not an integer.
///         `undefinedresult` if int2 is zero.
pub fn op_mod(interp: &mut Interpreter) -> Result<(), PSError> {
    let b = match interp.pop()? {
        PSValue::Integer(n) => n,
        other => {
            interp.push(other);
            return Err(PSError::TypeCheck { expected: "integer", got: "non-integer" });
        }
    };
    let a = match interp.pop()? {
        PSValue::Integer(n) => n,
        other => {
            interp.push(other);
            return Err(PSError::TypeCheck { expected: "integer", got: "non-integer" });
        }
    };
    if b == 0 { return Err(PSError::UndefinedResult); }
    interp.push(PSValue::Integer(a % b)); // Rust `%` sign follows dividend — matches PS
    Ok(())
}

// ── Unary operators ───────────────────────────────────────────────────────────

/// `abs` — absolute value; preserves the numeric type.
///
/// Stack effect: `num → |num|`
pub fn op_abs(interp: &mut Interpreter) -> Result<(), PSError> {
    let result = match pop_num(interp)? {
        Num::Int(n)   => PSValue::Integer(n.wrapping_abs()),
        Num::Float(f) => PSValue::Float(f.abs()),
    };
    interp.push(result);
    Ok(())
}

/// `neg` — negate; preserves the numeric type.
///
/// Stack effect: `num → -num`
pub fn op_neg(interp: &mut Interpreter) -> Result<(), PSError> {
    let result = match pop_num(interp)? {
        Num::Int(n)   => PSValue::Integer(n.wrapping_neg()),
        Num::Float(f) => PSValue::Float(-f),
    };
    interp.push(result);
    Ok(())
}

/// `ceiling` — smallest integer ≥ num.
///
/// Stack effect: `num → ceiling`
/// Type rule: integer → integer (unchanged); real → real.
pub fn op_ceiling(interp: &mut Interpreter) -> Result<(), PSError> {
    let result = match pop_num(interp)? {
        Num::Int(n)   => PSValue::Integer(n),
        Num::Float(f) => PSValue::Float(f.ceil()),
    };
    interp.push(result);
    Ok(())
}

/// `floor` — largest integer ≤ num.
///
/// Stack effect: `num → floor`
/// Type rule: integer → integer (unchanged); real → real.
pub fn op_floor(interp: &mut Interpreter) -> Result<(), PSError> {
    let result = match pop_num(interp)? {
        Num::Int(n)   => PSValue::Integer(n),
        Num::Float(f) => PSValue::Float(f.floor()),
    };
    interp.push(result);
    Ok(())
}

/// `round` — nearest integer; ties go away from zero (Rust `f64::round` semantics).
///
/// Stack effect: `num → rounded`
/// Type rule: integer → integer (unchanged); real → real.
///
/// Note: the PostScript spec rounds ties to the nearest even integer (banker's
/// rounding), but this implementation uses half-away-from-zero for simplicity.
pub fn op_round(interp: &mut Interpreter) -> Result<(), PSError> {
    let result = match pop_num(interp)? {
        Num::Int(n)   => PSValue::Integer(n),
        Num::Float(f) => PSValue::Float(f.round()),
    };
    interp.push(result);
    Ok(())
}

/// `sqrt` — square root; result is always a float.
///
/// Stack effect: `num → real`
/// Errors: `rangecheck` if num is negative.
pub fn op_sqrt(interp: &mut Interpreter) -> Result<(), PSError> {
    let f = match pop_num(interp)? {
        Num::Int(n)   => n as f64,
        Num::Float(f) => f,
    };
    if f < 0.0 { return Err(PSError::RangeCheck); }
    interp.push(PSValue::Float(f.sqrt()));
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn push_int(interp: &mut Interpreter, n: i64) {
        interp.push(PSValue::Integer(n));
    }
    fn push_float(interp: &mut Interpreter, f: f64) {
        interp.push(PSValue::Float(f));
    }
    fn pop_int(interp: &mut Interpreter) -> i64 {
        match interp.pop().unwrap() {
            PSValue::Integer(n) => n,
            other => panic!("expected integer, got: {other}"),
        }
    }
    fn pop_float(interp: &mut Interpreter) -> f64 {
        match interp.pop().unwrap() {
            PSValue::Float(f) => f,
            other => panic!("expected float, got: {other}"),
        }
    }

    // ── add ──────────────────────────────────────────────────────────────────

    #[test]
    fn add_int_int() {
        let mut i = Interpreter::new();
        push_int(&mut i, 3); push_int(&mut i, 4);
        op_add(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 7);
    }

    #[test]
    fn add_int_float_promotes() {
        let mut i = Interpreter::new();
        push_int(&mut i, 3); push_float(&mut i, 1.5);
        op_add(&mut i).unwrap();
        assert!((pop_float(&mut i) - 4.5).abs() < f64::EPSILON);
    }

    #[test]
    fn add_float_float() {
        let mut i = Interpreter::new();
        push_float(&mut i, 1.1); push_float(&mut i, 2.2);
        op_add(&mut i).unwrap();
        assert!((pop_float(&mut i) - 3.3).abs() < 1e-10);
    }

    #[test]
    fn add_negative() {
        let mut i = Interpreter::new();
        push_int(&mut i, -5); push_int(&mut i, 3);
        op_add(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), -2);
    }

    // ── sub ──────────────────────────────────────────────────────────────────

    #[test]
    fn sub_int_int() {
        let mut i = Interpreter::new();
        push_int(&mut i, 10); push_int(&mut i, 3);
        op_sub(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 7);
    }

    #[test]
    fn sub_float_promotes() {
        let mut i = Interpreter::new();
        push_int(&mut i, 5); push_float(&mut i, 1.5);
        op_sub(&mut i).unwrap();
        assert!((pop_float(&mut i) - 3.5).abs() < f64::EPSILON);
    }

    // ── mul ──────────────────────────────────────────────────────────────────

    #[test]
    fn mul_int_int() {
        let mut i = Interpreter::new();
        push_int(&mut i, 6); push_int(&mut i, 7);
        op_mul(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 42);
    }

    #[test]
    fn mul_float_promotes() {
        let mut i = Interpreter::new();
        push_int(&mut i, 3); push_float(&mut i, 2.5);
        op_mul(&mut i).unwrap();
        assert!((pop_float(&mut i) - 7.5).abs() < f64::EPSILON);
    }

    // ── div ──────────────────────────────────────────────────────────────────

    #[test]
    fn div_always_float() {
        let mut i = Interpreter::new();
        push_int(&mut i, 4); push_int(&mut i, 2);
        op_div(&mut i).unwrap();
        assert!((pop_float(&mut i) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn div_non_integer_result() {
        let mut i = Interpreter::new();
        push_int(&mut i, 1); push_int(&mut i, 3);
        op_div(&mut i).unwrap();
        let result = pop_float(&mut i);
        assert!((result - 1.0 / 3.0).abs() < 1e-15);
    }

    #[test]
    fn div_by_zero_errors() {
        let mut i = Interpreter::new();
        push_int(&mut i, 5); push_int(&mut i, 0);
        assert!(matches!(op_div(&mut i), Err(PSError::UndefinedResult)));
    }

    // ── idiv ─────────────────────────────────────────────────────────────────

    #[test]
    fn idiv_truncates_toward_zero() {
        let mut i = Interpreter::new();
        push_int(&mut i, 7); push_int(&mut i, 2);
        op_idiv(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 3);
    }

    #[test]
    fn idiv_negative_truncates_toward_zero() {
        // -7 / 2 = -3.5, truncated toward zero → -3
        let mut i = Interpreter::new();
        push_int(&mut i, -7); push_int(&mut i, 2);
        op_idiv(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), -3);
    }

    #[test]
    fn idiv_by_zero_errors() {
        let mut i = Interpreter::new();
        push_int(&mut i, 5); push_int(&mut i, 0);
        assert!(matches!(op_idiv(&mut i), Err(PSError::UndefinedResult)));
    }

    #[test]
    fn idiv_float_operand_errors() {
        let mut i = Interpreter::new();
        push_float(&mut i, 6.0); push_int(&mut i, 2);
        assert!(matches!(op_idiv(&mut i), Err(PSError::TypeCheck { .. })));
    }

    // ── mod ──────────────────────────────────────────────────────────────────

    #[test]
    fn mod_basic() {
        let mut i = Interpreter::new();
        push_int(&mut i, 7); push_int(&mut i, 3);
        op_mod(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 1);
    }

    #[test]
    fn mod_sign_follows_dividend() {
        // -7 mod 3 → -1  (dividend is negative)
        let mut i = Interpreter::new();
        push_int(&mut i, -7); push_int(&mut i, 3);
        op_mod(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), -1);
    }

    #[test]
    fn mod_by_zero_errors() {
        let mut i = Interpreter::new();
        push_int(&mut i, 5); push_int(&mut i, 0);
        assert!(matches!(op_mod(&mut i), Err(PSError::UndefinedResult)));
    }

    #[test]
    fn mod_float_operand_errors() {
        let mut i = Interpreter::new();
        push_int(&mut i, 5); push_float(&mut i, 2.0);
        assert!(matches!(op_mod(&mut i), Err(PSError::TypeCheck { .. })));
    }

    // ── abs ──────────────────────────────────────────────────────────────────

    #[test]
    fn abs_negative_int() {
        let mut i = Interpreter::new();
        push_int(&mut i, -9);
        op_abs(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 9);
    }

    #[test]
    fn abs_positive_int_unchanged() {
        let mut i = Interpreter::new();
        push_int(&mut i, 9);
        op_abs(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 9);
    }

    #[test]
    fn abs_float() {
        let mut i = Interpreter::new();
        push_float(&mut i, -3.7);
        op_abs(&mut i).unwrap();
        assert!((pop_float(&mut i) - 3.7).abs() < f64::EPSILON);
    }

    // ── neg ──────────────────────────────────────────────────────────────────

    #[test]
    fn neg_int() {
        let mut i = Interpreter::new();
        push_int(&mut i, 5);
        op_neg(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), -5);
    }

    #[test]
    fn neg_float() {
        let mut i = Interpreter::new();
        push_float(&mut i, 2.5);
        op_neg(&mut i).unwrap();
        assert!((pop_float(&mut i) + 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn neg_twice_is_identity() {
        let mut i = Interpreter::new();
        push_int(&mut i, 42);
        op_neg(&mut i).unwrap();
        op_neg(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 42);
    }

    // ── ceiling ──────────────────────────────────────────────────────────────

    #[test]
    fn ceiling_int_unchanged() {
        let mut i = Interpreter::new();
        push_int(&mut i, 5);
        op_ceiling(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 5);
    }

    #[test]
    fn ceiling_float_rounds_up() {
        let mut i = Interpreter::new();
        push_float(&mut i, 3.2);
        op_ceiling(&mut i).unwrap();
        assert!((pop_float(&mut i) - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ceiling_negative_float() {
        let mut i = Interpreter::new();
        push_float(&mut i, -3.7);
        op_ceiling(&mut i).unwrap();
        assert!((pop_float(&mut i) + 3.0).abs() < f64::EPSILON);
    }

    // ── floor ────────────────────────────────────────────────────────────────

    #[test]
    fn floor_int_unchanged() {
        let mut i = Interpreter::new();
        push_int(&mut i, 5);
        op_floor(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 5);
    }

    #[test]
    fn floor_float_rounds_down() {
        let mut i = Interpreter::new();
        push_float(&mut i, 3.9);
        op_floor(&mut i).unwrap();
        assert!((pop_float(&mut i) - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn floor_negative_float() {
        let mut i = Interpreter::new();
        push_float(&mut i, -3.2);
        op_floor(&mut i).unwrap();
        assert!((pop_float(&mut i) + 4.0).abs() < f64::EPSILON);
    }

    // ── round ────────────────────────────────────────────────────────────────

    #[test]
    fn round_int_unchanged() {
        let mut i = Interpreter::new();
        push_int(&mut i, 7);
        op_round(&mut i).unwrap();
        assert_eq!(pop_int(&mut i), 7);
    }

    #[test]
    fn round_float_down() {
        let mut i = Interpreter::new();
        push_float(&mut i, 3.4);
        op_round(&mut i).unwrap();
        assert!((pop_float(&mut i) - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn round_float_up() {
        let mut i = Interpreter::new();
        push_float(&mut i, 3.6);
        op_round(&mut i).unwrap();
        assert!((pop_float(&mut i) - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn round_half_away_from_zero() {
        // 0.5 rounds to 1.0 (half-away-from-zero)
        let mut i = Interpreter::new();
        push_float(&mut i, 0.5);
        op_round(&mut i).unwrap();
        assert!((pop_float(&mut i) - 1.0).abs() < f64::EPSILON);
    }

    // ── sqrt ─────────────────────────────────────────────────────────────────

    #[test]
    fn sqrt_perfect_square_int() {
        let mut i = Interpreter::new();
        push_int(&mut i, 9);
        op_sqrt(&mut i).unwrap();
        assert!((pop_float(&mut i) - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn sqrt_float_input() {
        let mut i = Interpreter::new();
        push_float(&mut i, 2.0);
        op_sqrt(&mut i).unwrap();
        assert!((pop_float(&mut i) - std::f64::consts::SQRT_2).abs() < 1e-15);
    }

    #[test]
    fn sqrt_zero() {
        let mut i = Interpreter::new();
        push_int(&mut i, 0);
        op_sqrt(&mut i).unwrap();
        assert!((pop_float(&mut i)).abs() < f64::EPSILON);
    }

    #[test]
    fn sqrt_negative_errors() {
        let mut i = Interpreter::new();
        push_int(&mut i, -1);
        assert!(matches!(op_sqrt(&mut i), Err(PSError::RangeCheck)));
    }

    // ── typecheck on non-numbers ──────────────────────────────────────────────

    #[test]
    fn add_non_number_errors() {
        let mut i = Interpreter::new();
        i.push(PSValue::Boolean(true));
        i.push(PSValue::Integer(1));
        assert!(matches!(op_add(&mut i), Err(PSError::TypeCheck { .. })));
    }
}
