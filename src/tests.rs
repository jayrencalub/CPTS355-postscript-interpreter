// ── Comprehensive test suite ───────────────────────────────────────────────────
//
// This file is a standalone, cross-cutting test suite that complements the
// per-module unit tests already written inside each ops/*.rs file.
//
// Sections:
//
//   1. stack_underflow   — every operator that pops, on an empty / too-short stack.
//   2. division_by_zero  — undefinedresult and rangecheck paths.
//   3. arithmetic_edges  — type preservation, truncation direction, sign rules.
//   4. string_boundaries — boundary conditions for get, getinterval, putinterval.
//   5. scoping           — dynamic shadowing, nesting, lexical scope subtleties.
//   6. comparison_edges  — string equality vs. array identity, bitwise edge cases.
//   7. control_flow      — deeper if/ifelse/repeat/for scenarios.
//   8. stack_ops         — exch/copy/count edge cases.
//   9. dict_ops          — reference sharing, overwrite, maxlength.
//  10. io_repr           — nested structures, binary bytes, = vs == for every type.
//  11. integration       — multi-operator chains that exercise several modules.

#[cfg(test)]
mod comprehensive {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::io::Write;
    use std::rc::Rc;

    use crate::interpreter::Interpreter;
    use crate::types::{PSError, PSString, PSValue};

    // ── value constructors ────────────────────────────────────────────────────

    fn int(n: i64)    -> PSValue { PSValue::Integer(n) }
    fn flt(f: f64)    -> PSValue { PSValue::Float(f) }
    fn bool_(b: bool) -> PSValue { PSValue::Boolean(b) }
    fn str_(s: &[u8]) -> PSValue { PSValue::String(PSString::new(s.to_vec())) }
    fn name(s: &str)  -> PSValue { PSValue::Name(Rc::from(s)) }
    fn xname(s: &str) -> PSValue { PSValue::ExecutableName(Rc::from(s)) }

    fn empty_proc() -> PSValue {
        PSValue::Procedure(Rc::new(vec![]), None)
    }

    // ── pop helpers ───────────────────────────────────────────────────────────

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
    fn pop_bool(i: &mut Interpreter) -> bool {
        match i.pop().unwrap() {
            PSValue::Boolean(b) => b,
            other => panic!("expected Boolean, got: {other}"),
        }
    }

    // ── I/O capture ───────────────────────────────────────────────────────────

    struct SharedBuf(Rc<RefCell<Vec<u8>>>);
    impl Write for SharedBuf {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    fn make_io_interp() -> (Interpreter, Rc<RefCell<Vec<u8>>>) {
        let buf = Rc::new(RefCell::new(Vec::<u8>::new()));
        let i = Interpreter::with_output(Box::new(SharedBuf(Rc::clone(&buf))));
        (i, buf)
    }
    fn captured(buf: &Rc<RefCell<Vec<u8>>>) -> String {
        String::from_utf8(buf.borrow().clone()).unwrap()
    }

    // ── dict helper ───────────────────────────────────────────────────────────

    fn new_dict_val() -> PSValue {
        PSValue::Dictionary(Rc::new(RefCell::new(HashMap::new())))
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 1. STACK UNDERFLOW — every pop-operator on an empty / too-short stack
    // ─────────────────────────────────────────────────────────────────────────

    mod stack_underflow {
        use super::*;
        use crate::ops::{arithmetic::*, comparison::*, control::*, dict::*, io::*, stack::*};
        use crate::ops::string::{op_get, op_getinterval, op_putinterval};
        use crate::ops::string::op_length as str_length;

        macro_rules! underflow_empty {
            ($name:ident, $op:expr) => {
                #[test]
                fn $name() {
                    let mut i = Interpreter::new();
                    assert!(matches!($op(&mut i), Err(PSError::StackUnderflow)),
                            "expected StackUnderflow from {}", stringify!($op));
                }
            };
        }

        // Arithmetic
        underflow_empty!(add_empty,     op_add);
        underflow_empty!(sub_empty,     op_sub);
        underflow_empty!(mul_empty,     op_mul);
        underflow_empty!(div_empty,     op_div);
        underflow_empty!(idiv_empty,    op_idiv);
        underflow_empty!(mod_empty,     op_mod);
        underflow_empty!(abs_empty,     op_abs);
        underflow_empty!(neg_empty,     op_neg);
        underflow_empty!(ceiling_empty, op_ceiling);
        underflow_empty!(floor_empty,   op_floor);
        underflow_empty!(round_empty,   op_round);
        underflow_empty!(sqrt_empty,    op_sqrt);

        // Comparison / logical
        underflow_empty!(eq_empty,  op_eq);
        underflow_empty!(ne_empty,  op_ne);
        underflow_empty!(lt_empty,  op_lt);
        underflow_empty!(le_empty,  op_le);
        underflow_empty!(gt_empty,  op_gt);
        underflow_empty!(ge_empty,  op_ge);
        underflow_empty!(and_empty, op_and);
        underflow_empty!(or_empty,  op_or);
        underflow_empty!(not_empty, op_not);

        // Control
        underflow_empty!(if_empty,      op_if);
        underflow_empty!(ifelse_empty,  op_ifelse);
        underflow_empty!(repeat_empty,  op_repeat);
        underflow_empty!(for_empty,     op_for);

        // Dict
        underflow_empty!(dict_empty,  op_dict);
        underflow_empty!(begin_empty, op_begin);
        underflow_empty!(def_empty,   op_def);

        // Stack
        underflow_empty!(dup_empty,  op_dup);
        underflow_empty!(exch_empty, op_exch);
        underflow_empty!(pop_empty,  op_pop);

        // I/O
        underflow_empty!(print_empty,        op_print);
        underflow_empty!(equal_empty,        op_equal);
        underflow_empty!(equal_equal_empty,  op_equal_equal);

        // String
        underflow_empty!(str_length_empty,   str_length);
        underflow_empty!(get_empty,          op_get);
        underflow_empty!(getinterval_empty,  op_getinterval);
        underflow_empty!(putinterval_empty,  op_putinterval);

        // Two-operand operators with exactly one element present.
        #[test]
        fn add_one_element() {
            let mut i = Interpreter::new();
            i.push(int(1));
            assert!(matches!(op_add(&mut i), Err(PSError::StackUnderflow)));
        }
        #[test]
        fn sub_one_element() {
            let mut i = Interpreter::new();
            i.push(int(1));
            assert!(matches!(op_sub(&mut i), Err(PSError::StackUnderflow)));
        }
        #[test]
        fn eq_one_element() {
            let mut i = Interpreter::new();
            i.push(int(1));
            assert!(matches!(op_eq(&mut i), Err(PSError::StackUnderflow)));
        }
        #[test]
        fn exch_one_element() {
            let mut i = Interpreter::new();
            i.push(int(1));
            assert!(matches!(op_exch(&mut i), Err(PSError::StackUnderflow)));
        }
        #[test]
        fn and_one_element() {
            let mut i = Interpreter::new();
            i.push(bool_(true));
            assert!(matches!(op_and(&mut i), Err(PSError::StackUnderflow)));
        }
        // `if` has its proc on top, bool below.  One element means bool is missing.
        #[test]
        fn if_proc_only_no_bool() {
            let mut i = Interpreter::new();
            i.push(empty_proc());
            assert!(matches!(op_if(&mut i), Err(PSError::StackUnderflow)));
        }
        // `ifelse` needs three items.  Two items means bool is missing.
        #[test]
        fn ifelse_two_procs_no_bool() {
            let mut i = Interpreter::new();
            i.push(empty_proc());
            i.push(empty_proc());
            assert!(matches!(op_ifelse(&mut i), Err(PSError::StackUnderflow)));
        }
        // `repeat` with only proc present (n missing).
        #[test]
        fn repeat_proc_only_no_count() {
            let mut i = Interpreter::new();
            i.push(empty_proc());
            assert!(matches!(op_repeat(&mut i), Err(PSError::StackUnderflow)));
        }
        // `def` with only one item (value present, key missing after pop).
        #[test]
        fn def_value_only_no_key() {
            let mut i = Interpreter::new();
            i.push(int(42));
            assert!(matches!(op_def(&mut i), Err(PSError::StackUnderflow)));
        }
        // `get` with only index; string is missing.
        #[test]
        fn get_index_only_no_string() {
            let mut i = Interpreter::new();
            i.push(int(0));
            assert!(matches!(op_get(&mut i), Err(PSError::StackUnderflow)));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 2. DIVISION BY ZERO AND UNDEFINED RESULT
    // ─────────────────────────────────────────────────────────────────────────

    mod division_by_zero {
        use super::*;
        use crate::ops::arithmetic::{op_div, op_idiv, op_mod, op_sqrt};

        #[test]
        fn div_int_by_zero() {
            let mut i = Interpreter::new();
            i.push(int(10)); i.push(int(0));
            assert!(matches!(op_div(&mut i), Err(PSError::UndefinedResult)));
        }
        #[test]
        fn div_float_by_zero() {
            let mut i = Interpreter::new();
            i.push(flt(5.0)); i.push(flt(0.0));
            assert!(matches!(op_div(&mut i), Err(PSError::UndefinedResult)));
        }
        #[test]
        fn div_negative_by_zero() {
            let mut i = Interpreter::new();
            i.push(int(-7)); i.push(int(0));
            assert!(matches!(op_div(&mut i), Err(PSError::UndefinedResult)));
        }
        #[test]
        fn idiv_by_zero() {
            let mut i = Interpreter::new();
            i.push(int(10)); i.push(int(0));
            assert!(matches!(op_idiv(&mut i), Err(PSError::UndefinedResult)));
        }
        #[test]
        fn idiv_negative_dividend_zero_divisor() {
            let mut i = Interpreter::new();
            i.push(int(-10)); i.push(int(0));
            assert!(matches!(op_idiv(&mut i), Err(PSError::UndefinedResult)));
        }
        #[test]
        fn mod_by_zero() {
            let mut i = Interpreter::new();
            i.push(int(7)); i.push(int(0));
            assert!(matches!(op_mod(&mut i), Err(PSError::UndefinedResult)));
        }
        #[test]
        fn sqrt_negative_float() {
            let mut i = Interpreter::new();
            i.push(flt(-1.0));
            assert!(matches!(op_sqrt(&mut i), Err(PSError::RangeCheck)));
        }
        #[test]
        fn sqrt_negative_integer() {
            let mut i = Interpreter::new();
            i.push(int(-4));
            assert!(matches!(op_sqrt(&mut i), Err(PSError::RangeCheck)));
        }
        // After a zero-division error, both operands have been consumed.
        #[test]
        fn div_by_zero_stack_empty_afterwards() {
            let mut i = Interpreter::new();
            i.push(int(5)); i.push(int(0));
            let _ = op_div(&mut i);
            assert_eq!(i.operand_stack.len(), 0);
        }
        #[test]
        fn idiv_by_zero_stack_empty_afterwards() {
            let mut i = Interpreter::new();
            i.push(int(5)); i.push(int(0));
            let _ = op_idiv(&mut i);
            assert_eq!(i.operand_stack.len(), 0);
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 3. ARITHMETIC — type preservation, truncation direction, sign rules
    // ─────────────────────────────────────────────────────────────────────────

    mod arithmetic_edges {
        use super::*;
        use crate::ops::arithmetic::*;

        // div always returns Float, even for exact division.
        #[test]
        fn div_exact_result_is_float() {
            let mut i = Interpreter::new();
            i.push(int(6)); i.push(int(2));
            op_div(&mut i).unwrap();
            assert!(matches!(i.pop().unwrap(), PSValue::Float(f) if (f - 3.0).abs() < 1e-12));
        }

        // add/sub/mul preserve int-int → int, promote when float present.
        #[test]
        fn add_int_int_result_is_int() {
            let mut i = Interpreter::new();
            i.push(int(2)); i.push(int(3));
            op_add(&mut i).unwrap();
            assert!(matches!(i.pop().unwrap(), PSValue::Integer(5)));
        }
        #[test]
        fn add_int_float_result_is_float() {
            let mut i = Interpreter::new();
            i.push(int(2)); i.push(flt(3.0));
            op_add(&mut i).unwrap();
            assert!(matches!(i.pop().unwrap(), PSValue::Float(_)));
        }
        #[test]
        fn sub_same_values_is_zero_int() {
            let mut i = Interpreter::new();
            i.push(int(42)); i.push(int(42));
            op_sub(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 0);
        }
        #[test]
        fn sub_gives_negative() {
            let mut i = Interpreter::new();
            i.push(int(3)); i.push(int(5));
            op_sub(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), -2);
        }
        #[test]
        fn mul_by_zero() {
            let mut i = Interpreter::new();
            i.push(int(99)); i.push(int(0));
            op_mul(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 0);
        }
        #[test]
        fn mul_by_one_identity() {
            let mut i = Interpreter::new();
            i.push(int(77)); i.push(int(1));
            op_mul(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 77);
        }
        #[test]
        fn mul_negative_negative_positive() {
            let mut i = Interpreter::new();
            i.push(int(-3)); i.push(int(-4));
            op_mul(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 12);
        }

        // idiv — truncation direction is toward zero, not toward -∞.
        #[test]
        fn idiv_positive_positive() {
            let mut i = Interpreter::new();
            i.push(int(7)); i.push(int(2));
            op_idiv(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 3);
        }
        #[test]
        fn idiv_negative_dividend_truncates_toward_zero() {
            let mut i = Interpreter::new();
            i.push(int(-7)); i.push(int(2));
            op_idiv(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), -3); // not -4
        }
        #[test]
        fn idiv_negative_divisor_truncates_toward_zero() {
            let mut i = Interpreter::new();
            i.push(int(7)); i.push(int(-2));
            op_idiv(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), -3); // not -4
        }
        #[test]
        fn idiv_both_negative() {
            let mut i = Interpreter::new();
            i.push(int(-7)); i.push(int(-2));
            op_idiv(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 3);
        }

        // mod — sign follows the dividend.
        #[test]
        fn mod_positive_positive() {
            let mut i = Interpreter::new();
            i.push(int(7)); i.push(int(3));
            op_mod(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 1);
        }
        #[test]
        fn mod_negative_dividend() {
            let mut i = Interpreter::new();
            i.push(int(-7)); i.push(int(3));
            op_mod(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), -1); // sign follows dividend
        }
        #[test]
        fn mod_negative_divisor() {
            let mut i = Interpreter::new();
            i.push(int(7)); i.push(int(-3));
            op_mod(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 1);
        }

        // neg / abs of zero.
        #[test]
        fn neg_zero_int() {
            let mut i = Interpreter::new();
            i.push(int(0)); op_neg(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 0);
        }
        #[test]
        fn abs_zero() {
            let mut i = Interpreter::new();
            i.push(int(0)); op_abs(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 0);
        }
        #[test]
        fn abs_preserves_int_type() {
            let mut i = Interpreter::new();
            i.push(int(-5)); op_abs(&mut i).unwrap();
            assert!(matches!(i.pop().unwrap(), PSValue::Integer(5)));
        }
        #[test]
        fn abs_preserves_float_type() {
            let mut i = Interpreter::new();
            i.push(flt(-5.0)); op_abs(&mut i).unwrap();
            assert!(matches!(i.pop().unwrap(), PSValue::Float(f) if (f - 5.0).abs() < 1e-12));
        }

        // ceiling / floor / round on negative fractions and integer passthrough.
        #[test]
        fn ceiling_negative_fraction() {
            let mut i = Interpreter::new();
            i.push(flt(-1.3)); op_ceiling(&mut i).unwrap();
            assert!((pop_flt(&mut i) - (-1.0)).abs() < 1e-12);
        }
        #[test]
        fn floor_negative_fraction() {
            let mut i = Interpreter::new();
            i.push(flt(-1.3)); op_floor(&mut i).unwrap();
            assert!((pop_flt(&mut i) - (-2.0)).abs() < 1e-12);
        }
        #[test]
        fn round_half_away_from_zero_positive() {
            let mut i = Interpreter::new();
            i.push(flt(0.5)); op_round(&mut i).unwrap();
            assert!((pop_flt(&mut i) - 1.0).abs() < 1e-12);
        }
        #[test]
        fn round_half_away_from_zero_negative() {
            let mut i = Interpreter::new();
            i.push(flt(-0.5)); op_round(&mut i).unwrap();
            assert!((pop_flt(&mut i) - (-1.0)).abs() < 1e-12);
        }
        #[test]
        fn ceiling_int_passes_through_unchanged() {
            let mut i = Interpreter::new();
            i.push(int(5)); op_ceiling(&mut i).unwrap();
            assert!(matches!(i.pop().unwrap(), PSValue::Integer(5)));
        }
        #[test]
        fn floor_int_passes_through_unchanged() {
            let mut i = Interpreter::new();
            i.push(int(5)); op_floor(&mut i).unwrap();
            assert!(matches!(i.pop().unwrap(), PSValue::Integer(5)));
        }
        #[test]
        fn round_int_passes_through_unchanged() {
            let mut i = Interpreter::new();
            i.push(int(5)); op_round(&mut i).unwrap();
            assert!(matches!(i.pop().unwrap(), PSValue::Integer(5)));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 4. STRING BOUNDARIES — get, getinterval, putinterval edge cases
    // ─────────────────────────────────────────────────────────────────────────

    mod string_boundaries {
        use super::*;
        use crate::ops::string::{op_get, op_getinterval, op_putinterval};
        use crate::ops::string::op_length as str_length;

        // get — valid and invalid indices on a 1-byte string.
        #[test]
        fn get_only_valid_index_in_single_byte_string() {
            let mut i = Interpreter::new();
            i.push(str_(b"Z")); i.push(int(0));
            op_get(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), b'Z' as i64);
        }
        #[test]
        fn get_index_equal_to_length_is_rangecheck() {
            let mut i = Interpreter::new();
            i.push(str_(b"Z")); i.push(int(1));
            assert!(matches!(op_get(&mut i), Err(PSError::RangeCheck)));
        }
        #[test]
        fn get_negative_index_is_rangecheck() {
            let mut i = Interpreter::new();
            i.push(str_(b"hello")); i.push(int(-1));
            assert!(matches!(op_get(&mut i), Err(PSError::RangeCheck)));
        }
        #[test]
        fn get_high_byte_255() {
            let mut i = Interpreter::new();
            i.push(str_(&[0xFF])); i.push(int(0));
            op_get(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 255);
        }
        #[test]
        fn get_into_substring_view_uses_view_offset() {
            // getinterval(b"hello", 1, 3) → view of b"ell"
            // get(0) on that view → b'e' (101)
            let mut i = Interpreter::new();
            i.push(str_(b"hello")); i.push(int(1)); i.push(int(3));
            op_getinterval(&mut i).unwrap();
            i.push(int(0));
            op_get(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), b'e' as i64);
        }

        // getinterval — boundary conditions.
        #[test]
        fn getinterval_full_string_is_alias() {
            let mut i = Interpreter::new();
            i.push(str_(b"hello")); i.push(int(0)); i.push(int(5));
            op_getinterval(&mut i).unwrap();
            match i.pop().unwrap() {
                PSValue::String(s) => { assert_eq!(s.to_bytes(), b"hello"); assert_eq!(s.len(), 5); }
                _ => panic!("expected String"),
            }
        }
        #[test]
        fn getinterval_empty_view_at_end_is_valid() {
            // start == length, count == 0 → valid empty view.
            let mut i = Interpreter::new();
            i.push(str_(b"abc")); i.push(int(3)); i.push(int(0));
            op_getinterval(&mut i).unwrap();
            match i.pop().unwrap() {
                PSValue::String(s) => assert_eq!(s.len(), 0),
                _ => panic!("expected String"),
            }
        }
        #[test]
        fn getinterval_one_past_end_is_rangecheck() {
            let mut i = Interpreter::new();
            i.push(str_(b"abc")); i.push(int(3)); i.push(int(1));
            assert!(matches!(op_getinterval(&mut i), Err(PSError::RangeCheck)));
        }
        #[test]
        fn getinterval_view_length_not_buffer_length() {
            let mut i = Interpreter::new();
            i.push(str_(b"hello world")); i.push(int(6)); i.push(int(5));
            op_getinterval(&mut i).unwrap();
            str_length(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 5); // view length, not buffer length 11
        }
        #[test]
        fn getinterval_start_zero_count_one() {
            let mut i = Interpreter::new();
            i.push(str_(b"hello")); i.push(int(0)); i.push(int(1));
            op_getinterval(&mut i).unwrap();
            match i.pop().unwrap() {
                PSValue::String(s) => assert_eq!(s.to_bytes(), b"h"),
                _ => panic!("expected String"),
            }
        }

        // putinterval — boundary conditions.
        #[test]
        fn putinterval_empty_src_is_noop() {
            let buf = Rc::new(RefCell::new(b"hello".to_vec()));
            let dest = PSValue::String(PSString { buf: Rc::clone(&buf), offset: 0, length: 5 });
            let mut i = Interpreter::new();
            i.push(dest); i.push(int(0)); i.push(str_(b""));
            op_putinterval(&mut i).unwrap();
            assert_eq!(&*buf.borrow(), b"hello");
        }
        #[test]
        fn putinterval_writes_exactly_last_byte() {
            let buf = Rc::new(RefCell::new(b"hello".to_vec()));
            let dest = PSValue::String(PSString { buf: Rc::clone(&buf), offset: 0, length: 5 });
            let mut i = Interpreter::new();
            i.push(dest); i.push(int(4)); i.push(str_(b"!"));
            op_putinterval(&mut i).unwrap();
            assert_eq!(&*buf.borrow(), b"hell!");
        }
        #[test]
        fn putinterval_one_past_last_byte_rangecheck() {
            // index=4, src len=2 → 4+2=6 > 5 → rangecheck.
            let buf = Rc::new(RefCell::new(b"hello".to_vec()));
            let dest = PSValue::String(PSString { buf: Rc::clone(&buf), offset: 0, length: 5 });
            let mut i = Interpreter::new();
            i.push(dest); i.push(int(4)); i.push(str_(b"XY"));
            assert!(matches!(op_putinterval(&mut i), Err(PSError::RangeCheck)));
        }
        #[test]
        fn putinterval_high_bytes() {
            let buf = Rc::new(RefCell::new(vec![0u8; 3]));
            let dest = PSValue::String(PSString { buf: Rc::clone(&buf), offset: 0, length: 3 });
            let mut i = Interpreter::new();
            i.push(dest); i.push(int(0)); i.push(str_(&[0xAB, 0xCD, 0xEF]));
            op_putinterval(&mut i).unwrap();
            assert_eq!(&*buf.borrow(), &[0xABu8, 0xCD, 0xEF]);
        }

        // Aliasing: mutation through substring is visible in the original.
        #[test]
        fn alias_write_visible_through_original() {
            let buf = Rc::new(RefCell::new(b"abcdefgh".to_vec()));
            let original = PSString { buf: Rc::clone(&buf), offset: 0, length: 8 };
            let alias    = PSString { buf: Rc::clone(&buf), offset: 2, length: 4 };
            let src = PSString::new(b"XXXX".to_vec());
            assert!(alias.put_interval(0, &src));
            assert_eq!(original.to_bytes(), b"abXXXXgh");
            assert_eq!(alias.to_bytes(),    b"XXXX");
        }
        #[test]
        fn alias_write_visible_through_getinterval_chain() {
            // Build alias via the operator stack.
            let buf = Rc::new(RefCell::new(b"hello world".to_vec()));
            let orig = PSValue::String(PSString { buf: Rc::clone(&buf), offset: 0, length: 11 });

            // get a substring for "world"
            let mut i = Interpreter::new();
            i.push(PSValue::String(PSString { buf: Rc::clone(&buf), offset: 0, length: 11 }));
            i.push(int(6)); i.push(int(5));
            op_getinterval(&mut i).unwrap();
            let sub = i.pop().unwrap();

            // putinterval on the substring
            i.push(sub); i.push(int(0)); i.push(str_(b"WORLD"));
            op_putinterval(&mut i).unwrap();

            // original string (kept separately) sees the change
            if let PSValue::String(s) = &orig {
                assert_eq!(s.to_bytes(), b"hello WORLD");
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 5. SCOPING — dynamic shadowing, end behaviour, lexical subtleties
    // ─────────────────────────────────────────────────────────────────────────

    mod scoping {
        use super::*;
        use crate::ops::dict::{op_begin, op_def, op_end};

        fn bind(i: &mut Interpreter, key: &str, val: PSValue) {
            i.push(name(key)); i.push(val);
            op_def(i).unwrap();
        }

        // Dynamic: newly pushed dict shadows an existing binding.
        #[test]
        fn dynamic_shadow_then_restore() {
            let mut i = Interpreter::new();
            bind(&mut i, "x", int(10));
            i.push(new_dict_val()); op_begin(&mut i).unwrap();
            bind(&mut i, "x", int(99));
            assert!(matches!(i.lookup_name("x", None), Some(PSValue::Integer(99))));
            op_end(&mut i).unwrap();
            assert!(matches!(i.lookup_name("x", None), Some(PSValue::Integer(10))));
        }

        // Dynamic: binding added inside begin/end is invisible after end.
        #[test]
        fn dynamic_binding_invisible_after_end() {
            let mut i = Interpreter::new();
            i.push(new_dict_val()); op_begin(&mut i).unwrap();
            bind(&mut i, "temp", int(42));
            op_end(&mut i).unwrap();
            assert!(i.lookup_name("temp", None).is_none());
        }

        // Dynamic: three-level nesting, all three names visible from deepest scope.
        #[test]
        fn dynamic_three_level_nesting() {
            let mut i = Interpreter::new();
            bind(&mut i, "a", int(1));
            i.push(new_dict_val()); op_begin(&mut i).unwrap();
            bind(&mut i, "b", int(2));
            i.push(new_dict_val()); op_begin(&mut i).unwrap();
            bind(&mut i, "c", int(3));
            assert!(i.lookup_name("a", None).is_some());
            assert!(i.lookup_name("b", None).is_some());
            assert!(i.lookup_name("c", None).is_some());
            op_end(&mut i).unwrap();
            assert!(i.lookup_name("c", None).is_none());
            assert!(i.lookup_name("b", None).is_some());
            op_end(&mut i).unwrap();
            assert!(i.lookup_name("b", None).is_none());
            assert!(i.lookup_name("a", None).is_some());
        }

        // Dynamic: overwriting an existing binding replaces it.
        #[test]
        fn dynamic_def_overwrites_existing() {
            let mut i = Interpreter::new();
            bind(&mut i, "x", int(1));
            bind(&mut i, "x", int(2));
            assert!(matches!(i.lookup_name("x", None), Some(PSValue::Integer(2))));
        }

        // end below base level raises an error.
        #[test]
        fn end_below_base_level_errors() {
            let mut i = Interpreter::new();
            assert!(op_end(&mut i).is_err());
        }

        // Lexical: captured scope does NOT see dicts pushed after the snapshot.
        #[test]
        fn lexical_does_not_see_later_begin() {
            let mut i = Interpreter::new();
            i.use_lexical_scope = true;
            bind(&mut i, "x", int(10));

            // Capture the scope.
            let proc = i.make_procedure(Rc::new(vec![]));
            let captured = match &proc {
                PSValue::Procedure(_, Some(sc)) => Rc::clone(sc),
                _ => panic!("expected captured scope"),
            };

            // Push a new dict that shadows x=20.
            i.push(new_dict_val()); op_begin(&mut i).unwrap();
            bind(&mut i, "x", int(20));

            // Live dynamic lookup sees 20.
            assert!(matches!(i.lookup_dynamic("x"), Some(PSValue::Integer(20))));
            // Lookup through the captured (pre-begin) scope sees 10.
            let via_capture = Interpreter::lookup_lexical(&captured, "x");
            assert!(matches!(via_capture, Some(PSValue::Integer(10))));
        }

        // Lexical: the snapshot holds Rc pointers — mutations to an already-
        // captured dict ARE visible through the snapshot (it's not a deep copy).
        #[test]
        fn lexical_snapshot_sees_mutations_to_captured_dict() {
            let mut i = Interpreter::new();
            i.use_lexical_scope = true;
            bind(&mut i, "x", int(10));

            let proc = i.make_procedure(Rc::new(vec![]));
            let captured = match &proc {
                PSValue::Procedure(_, Some(sc)) => Rc::clone(sc),
                _ => panic!(),
            };

            // Mutate x in userdict AFTER the snapshot.
            bind(&mut i, "x", int(99));

            // The snapshot still points to the same userdict Rc → mutation is visible.
            let via_capture = Interpreter::lookup_lexical(&captured, "x");
            assert!(matches!(via_capture, Some(PSValue::Integer(99))));
        }

        // Dynamic mode: make_procedure produces None scope.
        #[test]
        fn dynamic_make_procedure_no_captured_scope() {
            let mut i = Interpreter::new(); // use_lexical_scope = false
            let proc = i.make_procedure(Rc::new(vec![]));
            assert!(matches!(proc, PSValue::Procedure(_, None)));
        }

        // Lexical mode: make_procedure captures the current dict stack.
        #[test]
        fn lexical_make_procedure_captures_scope() {
            let mut i = Interpreter::new();
            i.use_lexical_scope = true;
            bind(&mut i, "x", int(42));
            let proc = i.make_procedure(Rc::new(vec![]));
            match proc {
                PSValue::Procedure(_, Some(scope)) => {
                    let found = Interpreter::lookup_lexical(&scope, "x");
                    assert!(matches!(found, Some(PSValue::Integer(42))));
                }
                _ => panic!("expected captured scope"),
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 6. COMPARISON AND LOGICAL — edge cases
    // ─────────────────────────────────────────────────────────────────────────

    mod comparison_edges {
        use super::*;
        use crate::ops::comparison::*;

        // eq — strings use VALUE equality; arrays use REFERENCE equality.
        #[test]
        fn eq_two_strings_same_content_true() {
            let mut i = Interpreter::new();
            i.push(str_(b"hello")); i.push(str_(b"hello"));
            op_eq(&mut i).unwrap();
            assert!(pop_bool(&mut i));
        }
        #[test]
        fn eq_two_arrays_same_content_different_rc_false() {
            let mut i = Interpreter::new();
            i.push(PSValue::Array(Rc::new(RefCell::new(vec![int(1)]))));
            i.push(PSValue::Array(Rc::new(RefCell::new(vec![int(1)]))));
            op_eq(&mut i).unwrap();
            assert!(!pop_bool(&mut i));
        }
        #[test]
        fn eq_two_arrays_same_rc_true() {
            let mut i = Interpreter::new();
            let a = PSValue::Array(Rc::new(RefCell::new(vec![])));
            i.push(a.clone()); i.push(a);
            op_eq(&mut i).unwrap();
            assert!(pop_bool(&mut i));
        }
        #[test]
        fn eq_null_null() {
            let mut i = Interpreter::new();
            i.push(PSValue::Null); i.push(PSValue::Null);
            op_eq(&mut i).unwrap();
            assert!(pop_bool(&mut i));
        }
        #[test]
        fn eq_empty_strings() {
            let mut i = Interpreter::new();
            i.push(str_(b"")); i.push(str_(b""));
            op_eq(&mut i).unwrap();
            assert!(pop_bool(&mut i));
        }

        // lt/gt with equal strings.
        #[test]
        fn lt_equal_strings_false() {
            let mut i = Interpreter::new();
            i.push(str_(b"abc")); i.push(str_(b"abc"));
            op_lt(&mut i).unwrap();
            assert!(!pop_bool(&mut i));
        }
        #[test]
        fn gt_equal_strings_false() {
            let mut i = Interpreter::new();
            i.push(str_(b"abc")); i.push(str_(b"abc"));
            op_gt(&mut i).unwrap();
            assert!(!pop_bool(&mut i));
        }
        // Shorter prefix string is lexicographically less.
        #[test]
        fn lt_prefix_is_less_than_full() {
            let mut i = Interpreter::new();
            i.push(str_(b"ab")); i.push(str_(b"abc"));
            op_lt(&mut i).unwrap();
            assert!(pop_bool(&mut i));
        }

        // and/or all-false cases.
        #[test]
        fn and_false_false_is_false() {
            let mut i = Interpreter::new();
            i.push(bool_(false)); i.push(bool_(false));
            op_and(&mut i).unwrap();
            assert!(!pop_bool(&mut i));
        }
        #[test]
        fn or_false_false_is_false() {
            let mut i = Interpreter::new();
            i.push(bool_(false)); i.push(bool_(false));
            op_or(&mut i).unwrap();
            assert!(!pop_bool(&mut i));
        }

        // Bitwise edge cases with i64 extremes.
        #[test]
        fn not_i64_max_gives_i64_min() {
            let mut i = Interpreter::new();
            i.push(int(i64::MAX)); op_not(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), i64::MIN);
        }
        #[test]
        fn not_i64_min_gives_i64_max() {
            let mut i = Interpreter::new();
            i.push(int(i64::MIN)); op_not(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), i64::MAX);
        }
        #[test]
        fn and_zero_anything_is_zero() {
            let mut i = Interpreter::new();
            i.push(int(0)); i.push(int(0xFFFF_FFFF));
            op_and(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 0);
        }
        #[test]
        fn or_minus_one_anything_is_minus_one() {
            let mut i = Interpreter::new();
            i.push(int(-1)); i.push(int(0));
            op_or(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), -1);
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 7. CONTROL FLOW — deeper scenarios
    // ─────────────────────────────────────────────────────────────────────────

    mod control_flow {
        use super::*;
        use crate::ops::control::*;

        fn push_proc<F>(f: F) -> PSValue
        where F: Fn(&mut Interpreter) -> Result<(), PSError> + 'static
        {
            PSValue::Procedure(Rc::new(vec![PSValue::Operator(Rc::new(f))]), None)
        }

        // if-false leaves stack perfectly clean.
        #[test]
        fn if_false_stack_empty() {
            let mut i = Interpreter::new();
            i.push(bool_(false));
            i.push(PSValue::Procedure(Rc::new(vec![int(42)]), None));
            op_if(&mut i).unwrap();
            assert_eq!(i.operand_stack.len(), 0);
        }

        // ifelse result ends up on stack.
        #[test]
        fn ifelse_result_on_stack() {
            let mut i = Interpreter::new();
            i.push(bool_(true));
            i.push(PSValue::Procedure(Rc::new(vec![int(100)]), None));
            i.push(PSValue::Procedure(Rc::new(vec![int(200)]), None));
            op_ifelse(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 100);
            assert_eq!(i.operand_stack.len(), 0);
        }

        // repeat — body sees and modifies the outer stack.
        #[test]
        fn repeat_doubles_top_three_times() {
            let double = Rc::new(|i: &mut Interpreter| -> Result<(), PSError> {
                let n = match i.pop()? {
                    PSValue::Integer(n) => n,
                    _ => return Err(PSError::TypeCheck { expected: "int", got: "other" }),
                };
                i.push(PSValue::Integer(n * 2)); Ok(())
            });
            let proc = PSValue::Procedure(Rc::new(vec![PSValue::Operator(double)]), None);
            let mut i = Interpreter::new();
            i.push(int(10)); i.push(int(3)); i.push(proc);
            op_repeat(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 80); // 10 * 2³
        }

        // for — sum 1..10 using an accumulator on the outer stack.
        #[test]
        fn for_sum_one_to_ten() {
            let add = Rc::new(|i: &mut Interpreter| -> Result<(), PSError> {
                let b = i.pop()?; let a = i.pop()?;
                match (a, b) {
                    (PSValue::Integer(x), PSValue::Integer(y)) => { i.push(PSValue::Integer(x + y)); Ok(()) }
                    _ => Err(PSError::TypeCheck { expected: "integer", got: "other" }),
                }
            });
            let proc = PSValue::Procedure(Rc::new(vec![PSValue::Operator(add)]), None);
            let mut i = Interpreter::new();
            i.push(int(0));  // accumulator
            i.push(int(1)); i.push(int(1)); i.push(int(10)); i.push(proc);
            op_for(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 55);
            assert_eq!(i.operand_stack.len(), 0);
        }

        // for descending — same sum, opposite direction.
        #[test]
        fn for_sum_descending_equals_ascending() {
            let add = Rc::new(|i: &mut Interpreter| -> Result<(), PSError> {
                let b = i.pop()?; let a = i.pop()?;
                match (a, b) {
                    (PSValue::Integer(x), PSValue::Integer(y)) => { i.push(PSValue::Integer(x + y)); Ok(()) }
                    _ => Err(PSError::TypeCheck { expected: "integer", got: "other" }),
                }
            });
            let proc = PSValue::Procedure(Rc::new(vec![PSValue::Operator(add)]), None);
            let mut i = Interpreter::new();
            i.push(int(0));
            i.push(int(10)); i.push(int(-1)); i.push(int(1)); i.push(proc);
            op_for(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 55);
        }

        // for — exactly one iteration when initial == limit.
        #[test]
        fn for_initial_equals_limit_one_iteration() {
            let mut i = Interpreter::new();
            i.push(int(5)); i.push(int(1)); i.push(int(5));
            i.push(empty_proc());
            op_for(&mut i).unwrap();
            assert_eq!(i.operand_stack.len(), 1); // only the pushed counter
            assert_eq!(pop_int(&mut i), 5);
        }

        // Nested if inside repeat.
        #[test]
        fn nested_if_in_repeat() {
            // 3 times: push true then run { 1 } if → three 1s total.
            let inner_proc = PSValue::Procedure(Rc::new(vec![int(1)]), None);
            let if_body = Rc::new(move |i: &mut Interpreter| -> Result<(), PSError> {
                i.push(bool_(true));
                i.push(inner_proc.clone());
                crate::ops::control::op_if(i)
            });
            let outer = PSValue::Procedure(Rc::new(vec![PSValue::Operator(if_body)]), None);
            let mut i = Interpreter::new();
            i.push(int(3)); i.push(outer);
            op_repeat(&mut i).unwrap();
            assert_eq!(i.operand_stack.len(), 3);
            assert_eq!(pop_int(&mut i), 1);
            assert_eq!(pop_int(&mut i), 1);
            assert_eq!(pop_int(&mut i), 1);
        }

        // quit propagates through exec_proc.
        #[test]
        fn quit_propagates_through_exec_proc() {
            let quit_op = Rc::new(|i: &mut Interpreter| crate::ops::control::op_quit(i));
            let proc = PSValue::Procedure(Rc::new(vec![PSValue::Operator(quit_op)]), None);
            let mut i = Interpreter::new();
            assert!(matches!(i.exec_proc(proc), Err(PSError::Quit)));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 8. STACK OPS — edge cases
    // ─────────────────────────────────────────────────────────────────────────

    mod stack_ops {
        use super::*;
        use crate::ops::stack::*;

        #[test]
        fn exch_is_own_inverse() {
            let mut i = Interpreter::new();
            i.push(int(1)); i.push(int(2));
            op_exch(&mut i).unwrap();
            op_exch(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 2);
            assert_eq!(pop_int(&mut i), 1);
        }
        #[test]
        fn pop_leaves_rest_intact() {
            let mut i = Interpreter::new();
            i.push(int(1)); i.push(int(2)); i.push(int(3));
            op_pop(&mut i).unwrap();
            assert_eq!(i.operand_stack.len(), 2);
            assert_eq!(pop_int(&mut i), 2);
            assert_eq!(pop_int(&mut i), 1);
        }
        #[test]
        fn copy_zero_is_noop_nonempty_stack() {
            let mut i = Interpreter::new();
            i.push(int(99)); i.push(int(0));
            op_copy(&mut i).unwrap();
            assert_eq!(i.operand_stack.len(), 1);
            assert_eq!(pop_int(&mut i), 99);
        }
        #[test]
        fn count_measures_depth_before_pushing() {
            let mut i = Interpreter::new();
            i.push(int(1)); i.push(int(2)); i.push(int(3));
            op_count(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 3);
            assert_eq!(i.operand_stack.len(), 3); // original three elements still there
        }
        #[test]
        fn clear_then_count_is_zero() {
            let mut i = Interpreter::new();
            i.push(int(1)); i.push(int(2));
            op_clear(&mut i).unwrap();
            op_count(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 0);
        }
        #[test]
        fn dup_shares_procedure_rc() {
            // dup on a Procedure clones the PSValue (Rc clone), so both
            // copies point to the same body Rc.
            let body = Rc::new(vec![int(1)]);
            let proc = PSValue::Procedure(Rc::clone(&body), None);
            let mut i = Interpreter::new();
            i.push(proc);
            op_dup(&mut i).unwrap();
            let a = i.pop().unwrap();
            let b = i.pop().unwrap();
            if let (PSValue::Procedure(ra, _), PSValue::Procedure(rb, _)) = (a, b) {
                assert!(Rc::ptr_eq(&ra, &rb));
            } else { panic!("expected two Procedures"); }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 9. DICT OPS — reference sharing, overwrite, maxlength
    // ─────────────────────────────────────────────────────────────────────────

    mod dict_ops {
        use super::*;
        use crate::ops::dict::{op_begin, op_def, op_dict, op_end, op_length as dict_length, op_maxlength};

        fn bind(i: &mut Interpreter, k: &str, v: PSValue) {
            i.push(name(k)); i.push(v); op_def(i).unwrap();
        }

        #[test]
        fn dict_zero_capacity_valid() {
            let mut i = Interpreter::new();
            i.push(int(0)); op_dict(&mut i).unwrap();
            assert!(matches!(i.pop().unwrap(), PSValue::Dictionary(_)));
        }
        #[test]
        fn length_after_three_defs() {
            let d = Rc::new(RefCell::new(HashMap::new()));
            let mut i = Interpreter::new();
            i.push(PSValue::Dictionary(Rc::clone(&d)));
            op_begin(&mut i).unwrap();
            bind(&mut i, "a", int(1));
            bind(&mut i, "b", int(2));
            bind(&mut i, "c", int(3));
            i.push(PSValue::Dictionary(Rc::clone(&d)));
            dict_length(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 3);
        }
        #[test]
        fn def_overwrite_does_not_increase_length() {
            let d = Rc::new(RefCell::new(HashMap::new()));
            let mut i = Interpreter::new();
            i.push(PSValue::Dictionary(Rc::clone(&d)));
            op_begin(&mut i).unwrap();
            bind(&mut i, "x", int(1));
            bind(&mut i, "x", int(2)); // overwrite
            i.push(PSValue::Dictionary(Rc::clone(&d)));
            dict_length(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 1);
        }
        #[test]
        fn dict_and_dict_stack_share_same_rc() {
            // After begin, a def through the dict stack mutates the same
            // underlying HashMap that the operand-stack copy references.
            let d = Rc::new(RefCell::new(HashMap::<String, PSValue>::new()));
            let mut i = Interpreter::new();
            i.push(PSValue::Dictionary(Rc::clone(&d)));
            i.push(PSValue::Dictionary(Rc::clone(&d))); // second copy stays on op-stack
            op_begin(&mut i).unwrap();
            bind(&mut i, "k", int(7));
            assert!(matches!(*d.borrow().get("k").unwrap(), PSValue::Integer(7)));
        }
        #[test]
        fn maxlength_at_least_capacity_hint() {
            let mut i = Interpreter::new();
            i.push(int(16)); op_dict(&mut i).unwrap();
            match i.pop().unwrap() {
                PSValue::Dictionary(d) => {
                    i.push(PSValue::Dictionary(d));
                    op_maxlength(&mut i).unwrap();
                    assert!(pop_int(&mut i) >= 16);
                }
                _ => panic!(),
            }
        }
        #[test]
        fn begin_non_dict_typecheck() {
            let mut i = Interpreter::new();
            i.push(int(5));
            assert!(matches!(op_begin(&mut i), Err(PSError::TypeCheck { .. })));
            // value pushed back
            assert_eq!(pop_int(&mut i), 5);
        }
        #[test]
        fn def_non_name_key_typecheck() {
            let mut i = Interpreter::new();
            i.push(int(42)); i.push(int(1));
            assert!(matches!(op_def(&mut i), Err(PSError::TypeCheck { .. })));
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 10. I/O REPRESENTATION — nested structures, binary bytes, = vs ==
    // ─────────────────────────────────────────────────────────────────────────

    mod io_repr {
        use super::*;
        use crate::ops::io::{op_equal, op_equal_equal, op_print};

        // print with binary (non-ASCII) bytes — raw bytes pass through unchanged.
        #[test]
        fn print_binary_bytes_pass_through() {
            let (mut i, buf) = make_io_interp();
            i.push(str_(&[0x01, 0x02, 0xFF]));
            op_print(&mut i).unwrap();
            assert_eq!(*buf.borrow(), vec![0x01u8, 0x02, 0xFF]);
        }

        // == high byte → octal escape.
        #[test]
        fn equal_equal_high_byte_octal_escaped() {
            let (mut i, buf) = make_io_interp();
            i.push(str_(&[0xFF])); // 255 = 0377 octal
            op_equal_equal(&mut i).unwrap();
            assert_eq!(captured(&buf), "(\\377)\n");
        }

        // == nested array.
        #[test]
        fn equal_equal_nested_array() {
            let (mut i, buf) = make_io_interp();
            let inner = PSValue::Array(Rc::new(RefCell::new(vec![int(2), int(3)])));
            let outer = PSValue::Array(Rc::new(RefCell::new(vec![int(1), inner])));
            i.push(outer);
            op_equal_equal(&mut i).unwrap();
            assert_eq!(captured(&buf), "[1 [2 3]]\n");
        }

        // == array containing a string — string shown with parens.
        #[test]
        fn equal_equal_array_with_string() {
            let (mut i, buf) = make_io_interp();
            let arr = PSValue::Array(Rc::new(RefCell::new(vec![str_(b"hi"), int(42)])));
            i.push(arr);
            op_equal_equal(&mut i).unwrap();
            assert_eq!(captured(&buf), "[(hi) 42]\n");
        }

        // == procedure containing a literal name.
        #[test]
        fn equal_equal_proc_with_literal_name() {
            let (mut i, buf) = make_io_interp();
            let proc = PSValue::Procedure(Rc::new(vec![name("foo"), int(1)]), None);
            i.push(proc);
            op_equal_equal(&mut i).unwrap();
            assert_eq!(captured(&buf), "{ /foo 1 }\n");
        }

        // == nested procedure.
        #[test]
        fn equal_equal_nested_procedure() {
            let (mut i, buf) = make_io_interp();
            let inner = PSValue::Procedure(Rc::new(vec![int(1)]), None);
            let outer = PSValue::Procedure(Rc::new(vec![inner, xname("exec")]), None);
            i.push(outer);
            op_equal_equal(&mut i).unwrap();
            assert_eq!(captured(&buf), "{ { 1 } exec }\n");
        }

        // = for a procedure shows the body (same as ==).
        #[test]
        fn equal_proc_shows_body() {
            let (mut i, buf) = make_io_interp();
            let proc = PSValue::Procedure(Rc::new(vec![int(1), xname("add")]), None);
            i.push(proc);
            op_equal(&mut i).unwrap();
            assert_eq!(captured(&buf), "{ 1 add }\n");
        }

        // = and == produce the same output for numbers and booleans.
        #[test]
        fn equal_equal_equal_same_for_numbers() {
            for v in [int(0), int(-1), flt(1.5), bool_(true), bool_(false), PSValue::Null] {
                let (mut i1, buf1) = make_io_interp();
                i1.push(v.clone()); op_equal(&mut i1).unwrap();
                let (mut i2, buf2) = make_io_interp();
                i2.push(v.clone()); op_equal_equal(&mut i2).unwrap();
                assert_eq!(captured(&buf1), captured(&buf2),
                    "= and == differ for {v}");
            }
        }

        // = strips parens from string; == keeps them.
        #[test]
        fn equal_vs_equal_equal_for_string() {
            let (mut i1, buf1) = make_io_interp();
            i1.push(str_(b"world")); op_equal(&mut i1).unwrap();
            let (mut i2, buf2) = make_io_interp();
            i2.push(str_(b"world")); op_equal_equal(&mut i2).unwrap();
            assert_eq!(captured(&buf1), "world\n");
            assert_eq!(captured(&buf2), "(world)\n");
        }

        // = strips / from literal name; == keeps it.
        #[test]
        fn equal_vs_equal_equal_for_literal_name() {
            let (mut i1, buf1) = make_io_interp();
            i1.push(name("foo")); op_equal(&mut i1).unwrap();
            let (mut i2, buf2) = make_io_interp();
            i2.push(name("foo")); op_equal_equal(&mut i2).unwrap();
            assert_eq!(captured(&buf1), "foo\n");
            assert_eq!(captured(&buf2), "/foo\n");
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 11. INTEGRATION — multi-operator chains
    // ─────────────────────────────────────────────────────────────────────────

    mod integration {
        use super::*;
        use crate::ops::arithmetic::{op_abs, op_add, op_mul, op_sub};
        use crate::ops::comparison::{op_eq, op_gt, op_not};
        use crate::ops::control::{op_if, op_ifelse, op_repeat};
        use crate::ops::dict::{op_def, op_begin, op_end};
        use crate::ops::stack::{op_dup, op_exch};
        use crate::ops::string::{op_get, op_getinterval, op_putinterval};

        // (3 + 4) * 5 = 35
        #[test]
        fn add_then_mul() {
            let mut i = Interpreter::new();
            i.push(int(3)); i.push(int(4)); op_add(&mut i).unwrap();
            i.push(int(5)); op_mul(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 35);
        }

        // |a - b| is symmetric
        #[test]
        fn abs_of_subtraction_symmetric() {
            let mut i1 = Interpreter::new();
            i1.push(int(3)); i1.push(int(10)); op_sub(&mut i1).unwrap(); op_abs(&mut i1).unwrap();
            let mut i2 = Interpreter::new();
            i2.push(int(10)); i2.push(int(3)); op_sub(&mut i2).unwrap(); op_abs(&mut i2).unwrap();
            assert_eq!(pop_int(&mut i1), pop_int(&mut i2));
        }

        // n dup mul = n²
        #[test]
        fn dup_mul_is_square() {
            let mut i = Interpreter::new();
            i.push(int(7)); op_dup(&mut i).unwrap(); op_mul(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 49);
        }

        // Comparison driving ifelse.
        #[test]
        fn comparison_drives_ifelse() {
            // 5 3 gt { 100 } { 200 } ifelse → 100
            let mut i = Interpreter::new();
            i.push(int(5)); i.push(int(3)); op_gt(&mut i).unwrap();
            i.push(PSValue::Procedure(Rc::new(vec![int(100)]), None));
            i.push(PSValue::Procedure(Rc::new(vec![int(200)]), None));
            op_ifelse(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 100);
        }

        // not not = identity for both booleans.
        #[test]
        fn double_not_is_identity() {
            for &b in &[true, false] {
                let mut i = Interpreter::new();
                i.push(bool_(b));
                op_not(&mut i).unwrap();
                op_not(&mut i).unwrap();
                assert_eq!(pop_bool(&mut i), b);
            }
        }

        // dup eq is always true for any scalar.
        #[test]
        fn dup_eq_always_true() {
            let values = [int(0), int(-1), flt(1.5), bool_(true), bool_(false),
                          str_(b"hi"), PSValue::Null];
            for v in values {
                let mut i = Interpreter::new();
                i.push(v.clone());
                op_dup(&mut i).unwrap();
                op_eq(&mut i).unwrap();
                assert!(pop_bool(&mut i), "dup eq failed for {v}");
            }
        }

        // def then conditional: x>0 → take true branch.
        #[test]
        fn def_then_gt_then_if() {
            let mut i = Interpreter::new();
            i.push(name("x")); i.push(int(5)); op_def(&mut i).unwrap();
            let x = i.lookup_name("x", None).unwrap();
            i.push(x); i.push(int(0)); op_gt(&mut i).unwrap();
            i.push(PSValue::Procedure(Rc::new(vec![int(42)]), None));
            op_if(&mut i).unwrap();
            assert_eq!(pop_int(&mut i), 42);
        }

        // def in nested scope, visible inside, invisible after end.
        #[test]
        fn def_scoped_visibility() {
            let mut i = Interpreter::new();
            i.push(new_dict_val()); op_begin(&mut i).unwrap();
            i.push(name("local")); i.push(int(7)); op_def(&mut i).unwrap();
            assert!(i.lookup_name("local", None).is_some());
            op_end(&mut i).unwrap();
            assert!(i.lookup_name("local", None).is_none());
        }

        // String aliasing chain: getinterval + putinterval + original visible.
        #[test]
        fn string_alias_chain() {
            let buf = Rc::new(RefCell::new(b"hello world".to_vec()));
            let s   = PSValue::String(PSString { buf: Rc::clone(&buf), offset: 0, length: 11 });
            let sub = PSValue::String(PSString { buf: Rc::clone(&buf), offset: 0, length: 5 });
            let mut i = Interpreter::new();
            i.push(sub); i.push(int(0)); i.push(str_(b"HELLO"));
            op_putinterval(&mut i).unwrap();
            if let PSValue::String(r) = &s { assert_eq!(r.to_bytes(), b"HELLO world"); }
        }

        // Fibonacci-style: compute 8th Fibonacci number using repeat + exch.
        // State: (prev, cur) on stack; each step: dup exch add → (cur, prev+cur)
        // F(1)=1, F(2)=1, F(3)=2, ..., F(8)=21
        // We run 6 more steps after initializing (1,1).
        #[test]
        fn fibonacci_via_repeat_exch_add() {
            let step = Rc::new(|i: &mut Interpreter| -> Result<(), PSError> {
                // Stack: [a, b]  →  [b, a+b]
                op_dup(i)?;                 // [a, b, b]
                let b = i.pop().unwrap();   // [a, b]; b saved
                op_exch(i)?;               // [b, a]
                let a = i.pop().unwrap();
                let bv = b.clone();
                i.push(bv);                 // [b]
                i.push(a.clone());
                i.push(b);
                op_add(i)                  // [b, a+b]
            });
            let proc = PSValue::Procedure(Rc::new(vec![PSValue::Operator(step)]), None);
            let mut i = Interpreter::new();
            i.push(int(1)); i.push(int(1)); // F(1), F(2)
            i.push(int(6)); i.push(proc);  // 6 more steps → F(8)
            op_repeat(&mut i).unwrap();
            let f8 = pop_int(&mut i);       // top of stack is F(8)
            assert_eq!(f8, 21);
        }

        // Verify that a for-loop counter is not left on the stack after the body
        // consumes it.
        #[test]
        fn for_body_consuming_counter_leaves_empty_stack() {
            let discard = Rc::new(|i: &mut Interpreter| -> Result<(), PSError> {
                i.pop()?; Ok(()) // consume the counter
            });
            let proc = PSValue::Procedure(Rc::new(vec![PSValue::Operator(discard)]), None);
            let mut i = Interpreter::new();
            i.push(int(1)); i.push(int(1)); i.push(int(5)); i.push(proc);
            crate::ops::control::op_for(&mut i).unwrap();
            assert_eq!(i.operand_stack.len(), 0);
        }
    }
}
