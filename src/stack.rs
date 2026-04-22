use crate::types::{PSError, PSValue};

/// The PostScript operand stack.
#[derive(Default, Debug)]
pub struct OperandStack(Vec<PSValue>);

impl OperandStack {
    pub fn push(&mut self, val: PSValue) {
        self.0.push(val);
    }

    pub fn pop(&mut self) -> Result<PSValue, PSError> {
        self.0.pop().ok_or(PSError::StackUnderflow)
    }

    pub fn peek(&self) -> Result<&PSValue, PSError> {
        self.0.last().ok_or(PSError::StackUnderflow)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Index from the top (0 = top).
    pub fn index(&self, n: usize) -> Result<&PSValue, PSError> {
        let len = self.0.len();
        len.checked_sub(n + 1)
            .and_then(|i| self.0.get(i))
            .ok_or(PSError::StackUnderflow)
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Expose the raw slice for `pstack` / debugging.
    pub fn as_slice(&self) -> &[PSValue] {
        &self.0
    }
}
