use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

// ── Shared dictionary reference ───────────────────────────────────────────────
//
// A `DictRef` is the in-memory representation of a single PostScript dictionary
// object.  Using `Rc<RefCell<…>>` gives us two things:
//
//   • `Rc`      — multiple owners can hold a reference to the same dict.
//                 When you push a dict onto the dict stack with `begin`, the
//                 operand-stack value AND the dict-stack frame both point to the
//                 SAME underlying HashMap — mutations through one are visible
//                 through the other (reference semantics, matching PostScript).
//
//   • `RefCell` — interior mutability: the HashMap can be modified even when
//                 only a shared `&DictRef` is available, which is required
//                 because the dict stack holds refs while operators mutate dicts.
//
// Defined here (in types.rs) so that both `dict_stack.rs` and the `Procedure`
// variant below can use the same type without a circular dependency.
pub type DictRef = Rc<RefCell<HashMap<String, PSValue>>>;

// ── String representation ─────────────────────────────────────────────────────
//
// PostScript strings are MUTABLE BYTE SEQUENCES with REFERENCE SEMANTICS.
// The key question when implementing them in Rust is:
//
//   "If two PSValues reference the same underlying string, does a
//    putinterval on one affect the other?"
//
// Answer: YES — and by design.
//
// In PostScript, `getinterval` returns a SUBSTRING ALIAS, not a copy.  The
// returned value shares the backing buffer with the original.  This is
// documented in the PLRM (PostScript Language Reference Manual):
//
//   "getinterval returns a new object that shares the same storage as the
//    subsequence of the original."
//
// Example:
//   (hello world) 0 5 getinterval   % → (hello), an ALIAS into the buffer
//   0 (HELLO) putinterval           % → modifies the buffer
//   % original string is now (HELLO world) — the alias changed it
//
// RUST DESIGN:
//
// `PSValue::String(PSString)` where `PSString` is:
//
//   struct PSString {
//       buf:    Rc<RefCell<Vec<u8>>>,   // shared backing buffer
//       offset: usize,                  // where this view starts
//       length: usize,                  // how many bytes this view exposes
//   }
//
// • `Rc` gives shared ownership — multiple `PSString` values can point to
//   the same `Vec<u8>`.  Cloning a `PSValue::String` clones the `Rc` pointer,
//   not the bytes.  Both clones see the same data.
//
// • `RefCell` gives interior mutability — `putinterval` can borrow the buffer
//   mutably even when only a shared `&PSString` is available.
//
// • `offset + length` define a window into the buffer.  A fresh string has
//   `offset = 0`, `length = buf.len()`.  `getinterval` returns a new `PSString`
//   with the SAME `Rc` but a shifted `offset` and smaller `length`.
//
// ALIASING SAFETY:
//
// Because `getinterval` shares the `Rc`, a `putinterval` on the substring
// WILL mutate bytes that are also visible through the original string — this
// is the CORRECT PostScript behaviour.
//
// One edge case: `putinterval` where the source and destination SHARE THE
// SAME `Rc` (self-overlapping copy).  A naive implementation would try to
// hold a `borrow()` of the source and a `borrow_mut()` of the dest at the
// same time through the same `RefCell`, which would PANIC at runtime.
// The fix: read all source bytes into a temporary `Vec` BEFORE acquiring
// the mutable borrow.  See `PSString::put_interval` for the implementation.

/// A PostScript string value — a view into a shared, mutable byte buffer.
///
/// See the module comment above for the full aliasing design rationale.
#[derive(Clone, Debug)]
pub struct PSString {
    /// The shared backing store.  Multiple `PSString` values can share this.
    pub buf: Rc<RefCell<Vec<u8>>>,
    /// Index into `buf` where this view begins.
    pub offset: usize,
    /// Number of bytes exposed by this view.
    pub length: usize,
}

impl PSString {
    /// Create a fresh, independently-owned string from a byte vector.
    /// `offset = 0`, `length = bytes.len()`.
    pub fn new(bytes: Vec<u8>) -> Self {
        let length = bytes.len();
        Self { buf: Rc::new(RefCell::new(bytes)), offset: 0, length }
    }

    /// Number of bytes in this view.
    pub fn len(&self) -> usize { self.length }

    pub fn is_empty(&self) -> bool { self.length == 0 }

    /// Read the byte at position `i` within this view (0-based).
    /// Returns `None` if `i >= self.length`.
    pub fn get_byte(&self, i: usize) -> Option<u8> {
        if i < self.length {
            Some(self.buf.borrow()[self.offset + i])
        } else {
            None
        }
    }

    /// Return a sub-view that shares the same backing buffer.
    ///
    /// This is the aliasing operation: the caller and the returned value share
    /// the same `Rc<RefCell<Vec<u8>>>`.  A `put_interval` through either view
    /// modifies bytes visible through both.
    ///
    /// Returns `None` if `start + count` would exceed this view's bounds.
    pub fn get_interval(&self, start: usize, count: usize) -> Option<Self> {
        if start + count <= self.length {
            Some(Self {
                buf: Rc::clone(&self.buf),  // shares the buffer — NOT a copy
                offset: self.offset + start,
                length: count,
            })
        } else {
            None
        }
    }

    /// Overwrite bytes in this view starting at `start` with bytes from `src`.
    ///
    /// Returns `false` (rangecheck) if `start + src.length > self.length`.
    ///
    /// ALIASING SAFETY: if `self` and `src` share the same `Rc` (e.g. `src`
    /// was produced by `get_interval` on `self`), a naive implementation would
    /// borrow the same `RefCell` both mutably (for writing) and immutably (for
    /// reading) simultaneously, which would panic at runtime.
    ///
    /// The fix: collect all source bytes into a temporary `Vec` while holding
    /// only an immutable borrow, then release it before acquiring the mutable
    /// borrow for writing.  This is always safe regardless of aliasing.
    pub fn put_interval(&self, start: usize, src: &PSString) -> bool {
        if start + src.length > self.length {
            return false;
        }
        // Phase 1 — read: collect source bytes while holding only an immutable borrow.
        // Dropping `src_bytes` releases the immutable borrow before phase 2.
        let src_bytes: Vec<u8> = {
            let b = src.buf.borrow();
            b[src.offset..src.offset + src.length].to_vec()
        };
        // Phase 2 — write: acquire the mutable borrow now that phase 1 is done.
        let dest_base = self.offset + start;
        let mut b = self.buf.borrow_mut();
        for (i, &byte) in src_bytes.iter().enumerate() {
            b[dest_base + i] = byte;
        }
        true
    }

    /// Copy the bytes of this view into a fresh `Vec<u8>`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let b = self.buf.borrow();
        b[self.offset..self.offset + self.length].to_vec()
    }
}

/// A PostScript value — the fundamental tagged union of the language.
#[derive(Clone)]
pub enum PSValue {
    Integer(i64),
    Float(f64),
    Boolean(bool),
    /// A PostScript string — a view into a shared, mutable byte buffer.
    /// See `PSString` and the module comment for aliasing semantics.
    String(PSString),
    /// Literal (non-executable) name, e.g. `/foo`. Pushed as a value.
    Name(Rc<str>),
    /// Executable name, e.g. `foo`. The interpreter looks this up and runs it.
    ExecutableName(Rc<str>),
    /// Mutable array heap object.
    Array(Rc<RefCell<Vec<PSValue>>>),
    /// Mutable dictionary heap object.  The inner type is `DictRef`.
    Dictionary(DictRef),

    // ── Scoping ──────────────────────────────────────────────────────────────
    //
    // A PostScript procedure `{ ... }` is represented here with two fields:
    //
    //   body  — the token sequence the interpreter will execute.
    //
    //   captured_scope — the dict-stack snapshot taken at the moment this
    //       procedure was pushed onto the operand stack (i.e. "defined").
    //
    //       None   → no snapshot; the interpreter uses DYNAMIC scoping,
    //                looking up names in the CURRENT dict stack at call time.
    //
    //       Some(env) → the interpreter uses LEXICAL scoping, looking up
    //                names in the CAPTURED env instead of the current stack.
    //
    // The `Rc` around the `Vec<DictRef>` means the captured snapshot is shared
    // cheaply between clones of the procedure value (e.g. stored under two
    // different names).  The individual DictRef entries inside are also Rc, so
    // the snapshot is a lightweight list of shared pointers — it does NOT copy
    // the HashMap contents.  This means mutations to an already-captured dict
    // (e.g. `def` in userdict after a procedure is defined) ARE still visible
    // through the snapshot.  What the snapshot PREVENTS is newly `begin`-pushed
    // dicts from being visible — they are simply not in the captured list.
    // That is exactly the observable difference shown in the scoping demo test.
    Procedure(Rc<Vec<PSValue>>, Option<Rc<Vec<DictRef>>>),
    /// The literal `null`.
    Null,
    /// A built-in operator backed by a Rust function.
    Operator(Rc<dyn Fn(&mut Interpreter) -> Result<(), PSError>>),
    /// A mark object (used by `mark`, `cleartomark`, etc.).
    Mark,
}

impl fmt::Display for PSValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PSValue::Integer(n) => write!(f, "{n}"),
            PSValue::Float(v) => write!(f, "{v}"),
            PSValue::Boolean(b) => write!(f, "{b}"),
            PSValue::String(s) => {
                write!(f, "({})", String::from_utf8_lossy(&s.to_bytes()))
            }
            PSValue::Name(n) => write!(f, "/{n}"),
            PSValue::ExecutableName(n) => write!(f, "{n}"),
            PSValue::Array(a) => {
                let items = a.borrow();
                let parts: Vec<String> = items.iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", parts.join(" "))
            }
            PSValue::Dictionary(_) => write!(f, "--dict--"),
            PSValue::Procedure(_, _) => write!(f, "--proc--"),
            PSValue::Null => write!(f, "null"),
            PSValue::Operator(_) => write!(f, "--operator--"),
            PSValue::Mark => write!(f, "--mark--"),
        }
    }
}

/// Runtime errors that operators can raise.
#[derive(Debug, thiserror::Error)]
pub enum PSError {
    #[error("stackunderflow")]
    StackUnderflow,
    #[error("typecheck: expected {expected}, got {got}")]
    TypeCheck { expected: &'static str, got: &'static str },
    #[error("undefined: {0}")]
    Undefined(String),
    #[error("rangecheck")]
    RangeCheck,
    #[error("dictfull")]
    DictFull,
    #[error("invalidaccess")]
    InvalidAccess,
    #[error("undefinedresult")]
    UndefinedResult,
    /// Raised by `quit` to signal the outer execution loop to exit cleanly.
    /// This is not a runtime error; callers should match on it separately.
    #[error("quit")]
    Quit,
    #[error("{0}")]
    Other(String),
}

impl fmt::Debug for PSValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PSValue::Operator(_) => write!(f, "Operator(..)"),
            other => write!(f, "{other}"),
        }
    }
}

// Re-exported so PSValue::Operator can name the type without a full path.
pub use crate::interpreter::Interpreter;
