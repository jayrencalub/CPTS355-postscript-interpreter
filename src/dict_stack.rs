// ── Dictionary stack ──────────────────────────────────────────────────────────
//
// The PostScript dictionary stack is the mechanism behind name lookup.  It is
// a Vec of DictRef frames; lookup walks from the TOP (most-recently pushed)
// toward the BOTTOM (systemdict) and returns the first match.
//
// Frame layout at interpreter startup:
//
//   index 0  →  systemdict  (built-in operators will be registered here)
//   index 1  →  userdict    (default scratch namespace for user programs)
//   index 2+ →  any dicts pushed via `begin`
//
// Because each frame is a `DictRef` (= Rc<RefCell<HashMap>>), the same
// dictionary object can appear simultaneously on the operand stack AND on the
// dict stack.  Mutations via `def` are visible through both references.

use crate::types::{DictRef, PSError, PSValue};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Create a new, empty dictionary with an optional pre-allocated capacity.
pub fn new_dict(capacity: usize) -> DictRef {
    Rc::new(RefCell::new(HashMap::with_capacity(capacity)))
}

/// The PostScript dictionary stack.
pub struct DictStack(Vec<DictRef>);

impl DictStack {
    /// Create a fresh stack with empty `systemdict` (index 0) and `userdict` (index 1).
    pub fn new() -> Self {
        Self(vec![new_dict(64), new_dict(64)])
    }

    // ── Dynamic lookup ────────────────────────────────────────────────────────
    //
    // DYNAMIC SCOPING: walk the CURRENT live stack from top to bottom and
    // return the first binding found.  The "current live stack" is whatever
    // dicts have been `begin`-pushed at the moment the lookup is performed —
    // completely independent of when or where the executing procedure was defined.

    /// Look up `key` in the current live dict stack (dynamic scoping).
    pub fn lookup(&self, key: &str) -> Option<PSValue> {
        Self::lookup_in(&self.0, key)
    }

    // ── Lexical lookup ────────────────────────────────────────────────────────
    //
    // LEXICAL SCOPING: walk a CAPTURED scope snapshot taken at the time a
    // procedure was *defined*, not at the time it is *called*.  Newly
    // `begin`-pushed dicts that did not exist when the procedure was defined
    // are invisible to this lookup — they are simply absent from the snapshot.

    /// Look up `key` in an arbitrary slice of dict frames (lexical scoping).
    ///
    /// Pass in the `captured_scope` stored inside a `PSValue::Procedure` to
    /// perform a lookup that respects the environment at definition time.
    pub fn lookup_in(frames: &[DictRef], key: &str) -> Option<PSValue> {
        // Walk from the last element (top of the captured stack) toward index 0.
        frames.iter().rev().find_map(|d| d.borrow().get(key).cloned())
    }

    // ── Mutation ──────────────────────────────────────────────────────────────

    /// Bind `key → val` in the **topmost** frame (implements the `def` operator).
    pub fn def(&mut self, key: String, val: PSValue) {
        self.0.last().unwrap().borrow_mut().insert(key, val);
    }

    // ── Stack manipulation ────────────────────────────────────────────────────

    /// Push `dict` onto the dict stack (implements `begin`).
    pub fn begin(&mut self, dict: DictRef) {
        self.0.push(dict);
    }

    /// Pop the topmost frame and return it (implements `end`).
    ///
    /// Returns `Err` if only the two base frames remain — PostScript does not
    /// allow removing `systemdict` or `userdict`.
    pub fn end(&mut self) -> Result<DictRef, PSError> {
        if self.0.len() <= 2 {
            return Err(PSError::Other("dictstackunderflow".into()));
        }
        Ok(self.0.pop().unwrap())
    }

    // ── Snapshot (for lexical scope capture) ─────────────────────────────────
    //
    // When `use_lexical_scope` is true, the interpreter calls `snapshot()`
    // every time it is about to push a procedure literal `{ … }` onto the
    // operand stack.  The snapshot is stored inside the `PSValue::Procedure`
    // and later used for name lookup when the procedure executes.
    //
    // The snapshot clones the Rc *pointers*, not the HashMap contents.  This
    // means the captured scope is a lightweight list of shared references — but
    // crucially, it does NOT include any dicts that are `begin`-pushed AFTER the
    // snapshot is taken.  That omission is exactly what makes lexical scoping
    // different from dynamic scoping.

    /// Return a `Vec` of `DictRef` pointers representing the current scope.
    /// Cheap: each element is an Rc clone (pointer copy, not HashMap clone).
    pub fn snapshot(&self) -> Vec<DictRef> {
        self.0.clone()
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    /// Number of frames currently on the stack.
    pub fn depth(&self) -> usize {
        self.0.len()
    }

    /// Reference to the topmost frame (used by `store` / `put`).
    pub fn top(&self) -> &DictRef {
        self.0.last().unwrap()
    }

    /// The `systemdict` frame (index 0).  Operators are registered here.
    pub fn systemdict(&self) -> &DictRef {
        &self.0[0]
    }

    /// The `userdict` frame (index 1).  Default user namespace.
    pub fn userdict(&self) -> &DictRef {
        &self.0[1]
    }
}

impl Default for DictStack {
    fn default() -> Self {
        Self::new()
    }
}
