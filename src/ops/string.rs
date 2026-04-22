// ── String operators ──────────────────────────────────────────────────────────
//
// PostScript strings are mutable byte sequences with reference semantics.
// The central aliasing question — "does putinterval through one PSValue affect
// another PSValue that shares the same backing buffer?" — is answered YES, and
// that answer is embedded in the PSString design in types.rs.  Read the large
// comment block there before reading this file.
//
// The four operators implemented here map directly onto PSString methods:
//
//   length       → PSString::len()
//   get          → PSString::get_byte(index)
//   getinterval  → PSString::get_interval(index, count)   ← creates an alias
//   putinterval  → PSString::put_interval(index, &src)    ← mutates through alias

use crate::interpreter::Interpreter;
use crate::types::{PSError, PSString, PSValue};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Pop the top of the operand stack and require it to be a string.
/// On type mismatch the value is pushed back before returning the error,
/// so the caller's stack is not silently consumed.
fn pop_string(interp: &mut Interpreter) -> Result<PSString, PSError> {
    match interp.pop()? {
        PSValue::String(s) => Ok(s),
        other => {
            interp.push(other);
            Err(PSError::TypeCheck { expected: "string", got: "non-string" })
        }
    }
}

/// Pop the top of the operand stack and require it to be a non-negative integer.
/// On type mismatch the value is pushed back.
fn pop_nonneg_int(interp: &mut Interpreter) -> Result<usize, PSError> {
    match interp.pop()? {
        PSValue::Integer(n) if n >= 0 => Ok(n as usize),
        PSValue::Integer(_) => Err(PSError::RangeCheck),
        other => {
            interp.push(other);
            Err(PSError::TypeCheck { expected: "integer", got: "non-integer" })
        }
    }
}

// ── operators ─────────────────────────────────────────────────────────────────

/// `length` — number of bytes in a string.
///
/// Stack effect: `string → int`
///
/// For a substring produced by `getinterval`, `length` returns the length of
/// the VIEW (i.e. the `count` passed to `getinterval`), not the length of the
/// underlying buffer.
pub fn op_length(interp: &mut Interpreter) -> Result<(), PSError> {
    let s = pop_string(interp)?;
    interp.push(PSValue::Integer(s.len() as i64));
    Ok(())
}

/// `get` — return the integer character code at a given index.
///
/// Stack effect: `string index → int`
///
/// PostScript strings are byte strings; `get` returns an integer in 0–255,
/// not a character.  Index is 0-based.
///
/// Errors: `rangecheck` if `index >= length`.
pub fn op_get(interp: &mut Interpreter) -> Result<(), PSError> {
    let index = pop_nonneg_int(interp)?;
    let s     = pop_string(interp)?;
    match s.get_byte(index) {
        Some(byte) => { interp.push(PSValue::Integer(byte as i64)); Ok(()) }
        None       => Err(PSError::RangeCheck),
    }
}

/// `getinterval` — return a substring that ALIASES the original buffer.
///
/// Stack effect: `string index count → substring`
///
/// ── Aliasing behaviour ────────────────────────────────────────────────────
///
/// The returned `PSValue::String` wraps a `PSString` that shares the SAME
/// `Rc<RefCell<Vec<u8>>>` as the source string.  No bytes are copied.
///
/// Consequence: if you later call `putinterval` on the returned substring,
/// the bytes written ARE VISIBLE through the original string, and vice-versa.
///
/// Example (PostScript):
///
///   /s (hello world) def
///   /sub s 0 5 getinterval def    % sub = (hello), shares s's buffer
///   sub 0 (HELLO) putinterval     % writes into the shared buffer
///   s =                           % prints (HELLO world) — original changed!
///
/// This is the correct PostScript semantics (PLRM §3.7).  It is also exactly
/// what happens in our Rust implementation because `PSString::get_interval`
/// does `Rc::clone(&self.buf)` rather than copying the bytes.
///
/// Errors: `rangecheck` if `index + count > length`.
pub fn op_getinterval(interp: &mut Interpreter) -> Result<(), PSError> {
    let count = pop_nonneg_int(interp)?;
    let index = pop_nonneg_int(interp)?;
    let s     = pop_string(interp)?;
    match s.get_interval(index, count) {
        Some(sub) => { interp.push(PSValue::String(sub)); Ok(()) }
        None      => Err(PSError::RangeCheck),
    }
}

/// `putinterval` — overwrite a range of bytes inside a string.
///
/// Stack effect: `dest index src → (nothing)`
///
/// Copies all bytes of `src` into `dest` starting at `index`.
/// `src.length` bytes are always written; the operator does not take a count.
///
/// ── Aliasing and self-overlap ─────────────────────────────────────────────
///
/// `dest` and `src` may share the same underlying buffer (e.g. when both were
/// produced from the same original string via `getinterval`).  The naive
/// approach — hold a `borrow()` of src while holding a `borrow_mut()` of dest
/// — would panic at runtime because both borrows would go through the same
/// `RefCell`.
///
/// The fix (implemented in `PSString::put_interval`):
///   1. Read all source bytes into a temporary `Vec<u8>`, releasing the
///      immutable borrow.
///   2. Acquire the mutable borrow and write the temporary bytes.
///
/// This is always safe regardless of whether the buffers alias.
///
/// Errors: `rangecheck` if `index + src.length > dest.length`.
pub fn op_putinterval(interp: &mut Interpreter) -> Result<(), PSError> {
    // Stack (bottom → top): dest  index  src
    let src   = pop_string(interp)?;
    let index = pop_nonneg_int(interp)?;
    let dest  = pop_string(interp)?;
    if dest.put_interval(index, &src) {
        Ok(())
    } else {
        Err(PSError::RangeCheck)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use std::cell::RefCell;

    fn make_string(s: &[u8]) -> PSValue {
        PSValue::String(PSString::new(s.to_vec()))
    }

    // ── op_length ─────────────────────────────────────────────────────────────

    #[test]
    fn length_of_ascii_string() {
        let mut i = Interpreter::new();
        i.push(make_string(b"hello"));
        op_length(&mut i).unwrap();
        assert!(matches!(i.pop().unwrap(), PSValue::Integer(5)));
    }

    #[test]
    fn length_of_empty_string() {
        let mut i = Interpreter::new();
        i.push(make_string(b""));
        op_length(&mut i).unwrap();
        assert!(matches!(i.pop().unwrap(), PSValue::Integer(0)));
    }

    #[test]
    fn length_of_getinterval_result_is_count_not_buffer_size() {
        // The view length (3) is returned, not the backing buffer length (5).
        let mut i = Interpreter::new();
        i.push(make_string(b"hello"));
        i.push(PSValue::Integer(1));
        i.push(PSValue::Integer(3));
        op_getinterval(&mut i).unwrap();
        op_length(&mut i).unwrap();
        assert!(matches!(i.pop().unwrap(), PSValue::Integer(3)));
    }

    #[test]
    fn length_non_string_errors_and_restores_stack() {
        let mut i = Interpreter::new();
        i.push(PSValue::Integer(42));
        assert!(matches!(op_length(&mut i), Err(PSError::TypeCheck { .. })));
        assert!(matches!(i.pop().unwrap(), PSValue::Integer(42)));
    }

    // ── op_get ────────────────────────────────────────────────────────────────

    #[test]
    fn get_returns_byte_value() {
        let mut i = Interpreter::new();
        i.push(make_string(b"ABC"));
        i.push(PSValue::Integer(0));
        op_get(&mut i).unwrap();
        assert!(matches!(i.pop().unwrap(), PSValue::Integer(65))); // b'A'
    }

    #[test]
    fn get_last_byte() {
        let mut i = Interpreter::new();
        i.push(make_string(b"ABC"));
        i.push(PSValue::Integer(2));
        op_get(&mut i).unwrap();
        assert!(matches!(i.pop().unwrap(), PSValue::Integer(67))); // b'C'
    }

    #[test]
    fn get_out_of_bounds_errors() {
        let mut i = Interpreter::new();
        i.push(make_string(b"AB"));
        i.push(PSValue::Integer(5));
        assert!(matches!(op_get(&mut i), Err(PSError::RangeCheck)));
    }

    #[test]
    fn get_into_getinterval_view() {
        // getinterval on (hello) at offset 1 count 3 → (ell)
        // get index 0 on the view → b'e'
        let mut i = Interpreter::new();
        i.push(make_string(b"hello"));
        i.push(PSValue::Integer(1));
        i.push(PSValue::Integer(3));
        op_getinterval(&mut i).unwrap();
        i.push(PSValue::Integer(0));
        op_get(&mut i).unwrap();
        assert!(matches!(i.pop().unwrap(), PSValue::Integer(101))); // b'e'
    }

    // ── op_getinterval ────────────────────────────────────────────────────────

    #[test]
    fn getinterval_returns_correct_bytes() {
        let mut i = Interpreter::new();
        i.push(make_string(b"hello world"));
        i.push(PSValue::Integer(6));
        i.push(PSValue::Integer(5));
        op_getinterval(&mut i).unwrap();
        match i.pop().unwrap() {
            PSValue::String(s) => assert_eq!(s.to_bytes(), b"world"),
            _ => panic!("expected String"),
        }
    }

    #[test]
    fn getinterval_zero_count_gives_empty_view() {
        let mut i = Interpreter::new();
        i.push(make_string(b"hello"));
        i.push(PSValue::Integer(2));
        i.push(PSValue::Integer(0));
        op_getinterval(&mut i).unwrap();
        match i.pop().unwrap() {
            PSValue::String(s) => assert_eq!(s.len(), 0),
            _ => panic!("expected String"),
        }
    }

    #[test]
    fn getinterval_out_of_bounds_errors() {
        let mut i = Interpreter::new();
        i.push(make_string(b"hi"));
        i.push(PSValue::Integer(1));
        i.push(PSValue::Integer(5)); // 1+5 > 2
        assert!(matches!(op_getinterval(&mut i), Err(PSError::RangeCheck)));
    }

    // ── putinterval ───────────────────────────────────────────────────────────

    #[test]
    fn putinterval_overwrites_bytes() {
        let mut i = Interpreter::new();
        i.push(make_string(b"hello world"));
        i.push(PSValue::Integer(6));
        i.push(make_string(b"WORLD"));
        op_putinterval(&mut i).unwrap();
        // The string is consumed by putinterval — re-push to check.
        // We need to hold a reference before passing.  Use a shared buf instead:
        let buf = Rc::new(RefCell::new(b"hello world".to_vec()));
        let dest = PSValue::String(PSString { buf: Rc::clone(&buf), offset: 0, length: 11 });
        let mut i2 = Interpreter::new();
        i2.push(dest);
        i2.push(PSValue::Integer(6));
        i2.push(make_string(b"WORLD"));
        op_putinterval(&mut i2).unwrap();
        assert_eq!(&*buf.borrow(), b"hello WORLD");
    }

    #[test]
    fn putinterval_at_start() {
        let buf = Rc::new(RefCell::new(b"....world".to_vec()));
        let dest = PSValue::String(PSString { buf: Rc::clone(&buf), offset: 0, length: 9 });
        let mut i = Interpreter::new();
        i.push(dest);
        i.push(PSValue::Integer(0));
        i.push(make_string(b"hell"));
        op_putinterval(&mut i).unwrap();
        assert_eq!(&*buf.borrow(), b"hellworld");
    }

    #[test]
    fn putinterval_out_of_bounds_errors() {
        let mut i = Interpreter::new();
        i.push(make_string(b"hi"));
        i.push(PSValue::Integer(1));
        i.push(make_string(b"XYZ")); // 1+3 > 2
        assert!(matches!(op_putinterval(&mut i), Err(PSError::RangeCheck)));
    }

    // ── aliasing: the central correctness test ────────────────────────────────
    //
    // This test verifies the aliasing contract: getinterval shares the buffer,
    // so putinterval on the substring changes bytes visible through the original.

    #[test]
    fn putinterval_through_alias_mutates_original() {
        // PostScript equivalent:
        //   /s (hello world) def
        //   /sub s 6 5 getinterval def   % sub = (world), aliased into s
        //   sub 0 (WORLD) putinterval    % write through the alias
        //   s = % → (hello WORLD)

        let buf = Rc::new(RefCell::new(b"hello world".to_vec()));

        // `original` and `alias` share the same Rc.
        let original = PSString { buf: Rc::clone(&buf), offset: 0, length: 11 };
        let alias    = PSString { buf: Rc::clone(&buf), offset: 6, length: 5  };

        // Write (WORLD) into the alias starting at position 0.
        let src = PSString::new(b"WORLD".to_vec());
        assert!(alias.put_interval(0, &src));

        // The mutation is visible through both the alias and the original.
        assert_eq!(alias.to_bytes(),    b"WORLD");
        assert_eq!(original.to_bytes(), b"hello WORLD");
    }

    #[test]
    fn putinterval_self_overlap_does_not_panic() {
        // src and dest share the same Rc (dest is the full buffer, src is a
        // sub-view).  This is the case that would panic without the
        // read-into-temp-Vec approach in PSString::put_interval.
        //
        // Copy the last 5 bytes of (abcde12345) over the first 5 bytes.
        // Expected: (12345 12345)  [space included for clarity; actual: no space]
        // Bytes: b"abcde12345" → copy [5..10] onto [0..5] → b"1234512345"

        let buf = Rc::new(RefCell::new(b"abcde12345".to_vec()));
        let dest = PSString { buf: Rc::clone(&buf), offset: 0, length: 10 };
        let src  = PSString { buf: Rc::clone(&buf), offset: 5, length: 5  };

        assert!(dest.put_interval(0, &src));
        assert_eq!(&*buf.borrow(), b"1234512345");
    }
}
