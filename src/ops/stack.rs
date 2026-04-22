use crate::interpreter::Interpreter;
use crate::types::{PSError, PSValue};

/// `dup` — duplicate the top element.
///
/// Stack effect: `a → a a`
pub fn op_dup(interp: &mut Interpreter) -> Result<(), PSError> {
    let top = interp.peek()?.clone();
    interp.push(top);
    Ok(())
}

/// `exch` — exchange the top two elements.
///
/// Stack effect: `a b → b a`
pub fn op_exch(interp: &mut Interpreter) -> Result<(), PSError> {
    let b = interp.pop()?; // top
    let a = interp.pop()?; // second
    interp.push(b);
    interp.push(a);
    Ok(())
}

/// `pop` — discard the top element.
///
/// Stack effect: `a → (nothing)`
pub fn op_pop(interp: &mut Interpreter) -> Result<(), PSError> {
    interp.pop()?;
    Ok(())
}

/// `copy` — copy the top *n* elements, where *n* is popped first.
///
/// Stack effect: `a₁ … aₙ n → a₁ … aₙ a₁ … aₙ`
///
/// Errors:
/// - `typecheck`    if the top element is not an integer.
/// - `rangecheck`   if *n* is negative.
/// - `stackunderflow` if fewer than *n* elements remain after popping *n*.
pub fn op_copy(interp: &mut Interpreter) -> Result<(), PSError> {
    let n = match interp.pop()? {
        PSValue::Integer(n) if n >= 0 => n as usize,
        PSValue::Integer(_) => return Err(PSError::RangeCheck),
        other => {
            // Put it back so the stack isn't silently consumed on a type error.
            interp.push(other);
            return Err(PSError::TypeCheck { expected: "integer", got: "non-integer" });
        }
    };

    let len = interp.operand_stack.len();
    if n > len {
        return Err(PSError::StackUnderflow);
    }

    // Clone the n topmost elements in bottom-to-top order so they are pushed
    // onto the stack in the same order, preserving relative position.
    let copies: Vec<PSValue> = interp.operand_stack.as_slice()[len - n..].to_vec();
    for v in copies {
        interp.push(v);
    }
    Ok(())
}

/// `clear` — remove every element from the operand stack.
///
/// Stack effect: `… → (empty)`
pub fn op_clear(interp: &mut Interpreter) -> Result<(), PSError> {
    interp.operand_stack.clear();
    Ok(())
}

/// `count` — push the number of elements currently on the stack.
///
/// Stack effect: `… → … n`
///
/// Note: the count reflects the depth *before* pushing `n`.
pub fn op_count(interp: &mut Interpreter) -> Result<(), PSError> {
    let n = interp.operand_stack.len() as i64;
    interp.push(PSValue::Integer(n));
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn interp_with(values: &[i64]) -> Interpreter {
        let mut interp = Interpreter::new();
        for &v in values {
            interp.push(PSValue::Integer(v));
        }
        interp
    }

    /// Collect the operand stack bottom-to-top as a Vec<i64>.
    /// Panics on any non-integer value — only used in integer-only tests.
    fn stack_ints(interp: &Interpreter) -> Vec<i64> {
        interp
            .operand_stack
            .as_slice()
            .iter()
            .map(|v| match v {
                PSValue::Integer(n) => *n,
                other => panic!("expected Integer, got: {other}"),
            })
            .collect()
    }

    // ── dup ──────────────────────────────────────────────────────────────────

    #[test]
    fn dup_duplicates_top() {
        let mut interp = interp_with(&[1, 2]);
        op_dup(&mut interp).unwrap();
        assert_eq!(stack_ints(&interp), vec![1, 2, 2]);
    }

    #[test]
    fn dup_single_element() {
        let mut interp = interp_with(&[42]);
        op_dup(&mut interp).unwrap();
        assert_eq!(stack_ints(&interp), vec![42, 42]);
    }

    #[test]
    fn dup_empty_stack_errors() {
        let mut interp = Interpreter::new();
        assert!(matches!(op_dup(&mut interp), Err(PSError::StackUnderflow)));
    }

    // ── exch ─────────────────────────────────────────────────────────────────

    #[test]
    fn exch_swaps_top_two() {
        // Stack [1, 2] (2 on top) → [2, 1] (1 on top)
        let mut interp = interp_with(&[1, 2]);
        op_exch(&mut interp).unwrap();
        assert_eq!(stack_ints(&interp), vec![2, 1]);
    }

    #[test]
    fn exch_three_elements_only_top_two_swapped() {
        // Stack [1, 2, 3] → [1, 3, 2]
        let mut interp = interp_with(&[1, 2, 3]);
        op_exch(&mut interp).unwrap();
        assert_eq!(stack_ints(&interp), vec![1, 3, 2]);
    }

    #[test]
    fn exch_one_element_errors() {
        let mut interp = interp_with(&[1]);
        assert!(matches!(op_exch(&mut interp), Err(PSError::StackUnderflow)));
    }

    #[test]
    fn exch_empty_stack_errors() {
        let mut interp = Interpreter::new();
        assert!(matches!(op_exch(&mut interp), Err(PSError::StackUnderflow)));
    }

    // ── pop ──────────────────────────────────────────────────────────────────

    #[test]
    fn pop_removes_top() {
        let mut interp = interp_with(&[1, 2, 3]);
        op_pop(&mut interp).unwrap();
        assert_eq!(stack_ints(&interp), vec![1, 2]);
    }

    #[test]
    fn pop_single_element_leaves_empty_stack() {
        let mut interp = interp_with(&[99]);
        op_pop(&mut interp).unwrap();
        assert!(interp.operand_stack.is_empty());
    }

    #[test]
    fn pop_empty_stack_errors() {
        let mut interp = Interpreter::new();
        assert!(matches!(op_pop(&mut interp), Err(PSError::StackUnderflow)));
    }

    // ── copy ─────────────────────────────────────────────────────────────────

    #[test]
    fn copy_duplicates_top_n_elements() {
        // Stack [1, 2, 3, 2] → [1, 2, 3, 2, 3]
        let mut interp = interp_with(&[1, 2, 3, 2]);
        op_copy(&mut interp).unwrap();
        assert_eq!(stack_ints(&interp), vec![1, 2, 3, 2, 3]);
    }

    #[test]
    fn copy_zero_is_noop() {
        let mut interp = interp_with(&[1, 2, 0]);
        op_copy(&mut interp).unwrap();
        assert_eq!(stack_ints(&interp), vec![1, 2]);
    }

    #[test]
    fn copy_all_elements() {
        let mut interp = interp_with(&[10, 20, 2]);
        op_copy(&mut interp).unwrap();
        assert_eq!(stack_ints(&interp), vec![10, 20, 10, 20]);
    }

    #[test]
    fn copy_negative_n_errors() {
        let mut interp = interp_with(&[1, -1]);
        assert!(matches!(op_copy(&mut interp), Err(PSError::RangeCheck)));
    }

    #[test]
    fn copy_non_integer_errors() {
        let mut interp = Interpreter::new();
        interp.push(PSValue::Boolean(true));
        assert!(matches!(op_copy(&mut interp), Err(PSError::TypeCheck { .. })));
        // The value should have been pushed back.
        assert_eq!(interp.operand_stack.len(), 1);
    }

    #[test]
    fn copy_n_exceeds_stack_errors() {
        let mut interp = interp_with(&[1, 5]); // only 1 real element, n=5
        assert!(matches!(op_copy(&mut interp), Err(PSError::StackUnderflow)));
    }

    // ── clear ────────────────────────────────────────────────────────────────

    #[test]
    fn clear_empties_stack() {
        let mut interp = interp_with(&[1, 2, 3]);
        op_clear(&mut interp).unwrap();
        assert!(interp.operand_stack.is_empty());
    }

    #[test]
    fn clear_on_empty_stack_is_ok() {
        let mut interp = Interpreter::new();
        assert!(op_clear(&mut interp).is_ok());
    }

    // ── count ────────────────────────────────────────────────────────────────

    #[test]
    fn count_returns_depth() {
        let mut interp = interp_with(&[10, 20, 30]);
        op_count(&mut interp).unwrap();
        // Stack: [10, 20, 30, 3]
        assert_eq!(stack_ints(&interp), vec![10, 20, 30, 3]);
    }

    #[test]
    fn count_on_empty_stack_returns_zero() {
        let mut interp = Interpreter::new();
        op_count(&mut interp).unwrap();
        assert_eq!(stack_ints(&interp), vec![0]);
    }

    #[test]
    fn count_does_not_disturb_other_elements() {
        let mut interp = interp_with(&[7]);
        op_count(&mut interp).unwrap();
        assert_eq!(stack_ints(&interp), vec![7, 1]);
    }

    // ── composition ──────────────────────────────────────────────────────────

    #[test]
    fn dup_then_exch_is_noop_on_equal_values() {
        // 5 dup exch → 5 5 → 5 5 (exch on identical values is visually a noop)
        let mut interp = interp_with(&[5]);
        op_dup(&mut interp).unwrap();
        op_exch(&mut interp).unwrap();
        assert_eq!(stack_ints(&interp), vec![5, 5]);
    }

    #[test]
    fn count_then_clear_then_count() {
        // count leaves n on stack, clear empties it, count then returns 0
        let mut interp = interp_with(&[1, 2, 3]);
        op_count(&mut interp).unwrap(); // [1, 2, 3, 3]
        op_clear(&mut interp).unwrap(); // []
        op_count(&mut interp).unwrap(); // [0]
        assert_eq!(stack_ints(&interp), vec![0]);
    }
}
