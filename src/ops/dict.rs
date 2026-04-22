// ── Dictionary operators ──────────────────────────────────────────────────────
//
// PostScript dictionaries are mutable, heap-allocated hash maps.  They serve
// two distinct roles:
//
//   1. DATA STRUCTURES — stored as `PSValue::Dictionary` on the operand stack,
//      passed to procedures, queried with `length`, etc.
//
//   2. SCOPE FRAMES — pushed onto the dict stack with `begin`/`end` so that
//      name lookup searches them automatically.
//
// Both roles use the same underlying `DictRef` (Rc<RefCell<HashMap>>), so
// mutations via `def` are always visible through all references to the same dict.

use std::rc::Rc;

use crate::dict_stack::new_dict;
use crate::interpreter::Interpreter;
use crate::types::{PSError, PSValue};

// ── Helper ────────────────────────────────────────────────────────────────────

/// Extract a name string from a `PSValue::Name` or `PSValue::ExecutableName`.
/// Used by `def` to accept both `/foo` (literal) and `foo` (executable) as keys.
fn name_string(v: PSValue) -> Result<String, PSError> {
    match v {
        PSValue::Name(n) | PSValue::ExecutableName(n) => Ok(n.to_string()),
        other => Err(PSError::TypeCheck {
            expected: "name",
            got: match other {
                PSValue::Integer(_)    => "integer",
                PSValue::Float(_)      => "real",
                PSValue::Boolean(_)    => "boolean",
                PSValue::String(_)     => "string",
                PSValue::Dictionary(_) => "dict",
                PSValue::Array(_)      => "array",
                PSValue::Procedure(..) => "procedure",
                PSValue::Null          => "null",
                PSValue::Operator(_)   => "operator",
                PSValue::Mark          => "mark",
                // Name variants already handled above
                PSValue::Name(_) | PSValue::ExecutableName(_) => unreachable!(),
            },
        }),
    }
}

// ── Operators ─────────────────────────────────────────────────────────────────

/// `dict` — create a new, empty dictionary.
///
/// Stack effect: `n → dict`
///
/// `n` is a capacity hint (how many entries the dict is expected to hold).
/// PostScript uses this to pre-allocate the hash map.  It is not a hard limit —
/// our implementation uses it as `HashMap::with_capacity(n)`.
///
/// Errors: `typecheck` if `n` is not an integer; `rangecheck` if `n < 0`.
pub fn op_dict(interp: &mut Interpreter) -> Result<(), PSError> {
    let n = match interp.pop()? {
        PSValue::Integer(n) if n >= 0 => n as usize,
        PSValue::Integer(_) => return Err(PSError::RangeCheck),
        other => {
            interp.push(other);
            return Err(PSError::TypeCheck { expected: "integer", got: "non-integer" });
        }
    };
    interp.push(PSValue::Dictionary(new_dict(n)));
    Ok(())
}

/// `length` — number of key/value pairs currently in a dictionary.
///
/// Stack effect: `dict → int`
///
/// Note: in full PostScript `length` is polymorphic (works on strings and arrays
/// too).  This implementation currently only handles dictionaries; a typecheck
/// error is raised for any other type.
///
/// Errors: `typecheck` if the top value is not a dictionary.
pub fn op_length(interp: &mut Interpreter) -> Result<(), PSError> {
    match interp.pop()? {
        PSValue::Dictionary(d) => {
            let n = d.borrow().len() as i64;
            interp.push(PSValue::Integer(n));
            Ok(())
        }
        other => {
            interp.push(other);
            Err(PSError::TypeCheck { expected: "dict", got: "non-dict" })
        }
    }
}

/// `maxlength` — the number of entries the dictionary can hold without rehashing.
///
/// Stack effect: `dict → int`
///
/// PostScript defines `maxlength` as the capacity hint passed to `dict`.  Our
/// implementation returns `HashMap::capacity()`, which is the actual allocated
/// capacity (always ≥ the hint due to load-factor rounding).
///
/// Errors: `typecheck` if the top value is not a dictionary.
pub fn op_maxlength(interp: &mut Interpreter) -> Result<(), PSError> {
    match interp.pop()? {
        PSValue::Dictionary(d) => {
            let n = d.borrow().capacity() as i64;
            interp.push(PSValue::Integer(n));
            Ok(())
        }
        other => {
            interp.push(other);
            Err(PSError::TypeCheck { expected: "dict", got: "non-dict" })
        }
    }
}

/// `begin` — push a dictionary onto the dictionary stack.
///
/// Stack effect: `dict → (nothing)`
///
/// After `begin`, the pushed dict becomes the CURRENT dictionary.  Subsequent
/// `def` calls bind names into this dict; name lookup searches it before any
/// lower frame.
///
/// Because the same `DictRef` is now referenced by both the dict stack and
/// wherever else the value was stored, all mutations through `def` are
/// immediately visible through every reference.
///
/// Errors: `typecheck` if the top value is not a dictionary.
pub fn op_begin(interp: &mut Interpreter) -> Result<(), PSError> {
    match interp.pop()? {
        PSValue::Dictionary(d) => {
            interp.dict_stack.begin(d);
            Ok(())
        }
        other => {
            interp.push(other);
            Err(PSError::TypeCheck { expected: "dict", got: "non-dict" })
        }
    }
}

/// `end` — pop the topmost dictionary from the dictionary stack.
///
/// Stack effect: `(nothing) → (nothing)`
///
/// Restores the previous current dictionary.  The popped dict is NOT pushed
/// back onto the operand stack (PostScript discards it).
///
/// Errors: `Other("dictstackunderflow")` if only the two base frames remain.
pub fn op_end(interp: &mut Interpreter) -> Result<(), PSError> {
    interp.dict_stack.end()?;
    Ok(())
}

/// `def` — bind a name to a value in the current (topmost) dictionary.
///
/// Stack effect: `key value → (nothing)`
///
/// `key` must be a literal name (`/foo`) or an executable name (`foo`).
/// `value` can be any PostScript object.
///
/// This is the fundamental binding operator.  Combined with `begin`/`end`, it
/// implements local variable scopes; at the top level it populates `userdict`.
///
/// Errors: `typecheck` if `key` is not a name; `stackunderflow` if fewer than
///         two elements are on the operand stack.
pub fn op_def(interp: &mut Interpreter) -> Result<(), PSError> {
    // PostScript calling convention: `/key value def`
    // Stack (bottom → top): … key value
    // So `value` is on top, `key` is second.
    let value = interp.pop()?;
    let key   = interp.pop()?;
    let name  = name_string(key)?;
    interp.dict_stack.def(name, value);
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict_stack::DictStack;
    use std::rc::Rc;

    // ── op_dict ───────────────────────────────────────────────────────────────

    #[test]
    fn dict_creates_empty_dictionary() {
        let mut i = Interpreter::new();
        i.push(PSValue::Integer(5));
        op_dict(&mut i).unwrap();
        match i.pop().unwrap() {
            PSValue::Dictionary(d) => assert_eq!(d.borrow().len(), 0),
            _ => panic!("expected Dictionary"),
        }
    }

    #[test]
    fn dict_negative_capacity_errors() {
        let mut i = Interpreter::new();
        i.push(PSValue::Integer(-1));
        assert!(matches!(op_dict(&mut i), Err(PSError::RangeCheck)));
    }

    #[test]
    fn dict_non_integer_errors() {
        let mut i = Interpreter::new();
        i.push(PSValue::Boolean(true));
        assert!(matches!(op_dict(&mut i), Err(PSError::TypeCheck { .. })));
    }

    // ── op_length ─────────────────────────────────────────────────────────────

    #[test]
    fn length_empty_dict() {
        let mut i = Interpreter::new();
        i.push(PSValue::Dictionary(new_dict(4)));
        op_length(&mut i).unwrap();
        assert!(matches!(i.pop().unwrap(), PSValue::Integer(0)));
    }

    #[test]
    fn length_after_insertions() {
        let mut i = Interpreter::new();
        let d = new_dict(4);
        d.borrow_mut().insert("a".into(), PSValue::Integer(1));
        d.borrow_mut().insert("b".into(), PSValue::Integer(2));
        i.push(PSValue::Dictionary(d));
        op_length(&mut i).unwrap();
        assert!(matches!(i.pop().unwrap(), PSValue::Integer(2)));
    }

    #[test]
    fn length_non_dict_errors() {
        let mut i = Interpreter::new();
        i.push(PSValue::Integer(42));
        assert!(matches!(op_length(&mut i), Err(PSError::TypeCheck { .. })));
        // Value must be pushed back so the stack is not silently consumed.
        assert!(matches!(i.pop().unwrap(), PSValue::Integer(42)));
    }

    // ── op_maxlength ──────────────────────────────────────────────────────────

    #[test]
    fn maxlength_gte_capacity_hint() {
        let mut i = Interpreter::new();
        i.push(PSValue::Integer(10));
        op_dict(&mut i).unwrap();           // creates dict with capacity hint 10
        op_maxlength(&mut i).unwrap();
        match i.pop().unwrap() {
            PSValue::Integer(n) => assert!(n >= 10),
            _ => panic!("expected Integer"),
        }
    }

    // ── op_begin / op_end ─────────────────────────────────────────────────────

    #[test]
    fn begin_pushes_dict_onto_dict_stack() {
        let mut i = Interpreter::new();
        assert_eq!(i.dict_stack.depth(), 2); // systemdict + userdict
        i.push(PSValue::Dictionary(new_dict(4)));
        op_begin(&mut i).unwrap();
        assert_eq!(i.dict_stack.depth(), 3);
    }

    #[test]
    fn end_pops_dict_from_dict_stack() {
        let mut i = Interpreter::new();
        i.push(PSValue::Dictionary(new_dict(4)));
        op_begin(&mut i).unwrap();
        assert_eq!(i.dict_stack.depth(), 3);
        op_end(&mut i).unwrap();
        assert_eq!(i.dict_stack.depth(), 2);
    }

    #[test]
    fn end_at_base_level_errors() {
        let mut i = Interpreter::new();
        // Only systemdict + userdict remain — end must not remove them.
        assert!(op_end(&mut i).is_err());
    }

    #[test]
    fn begin_non_dict_errors() {
        let mut i = Interpreter::new();
        i.push(PSValue::Integer(99));
        assert!(matches!(op_begin(&mut i), Err(PSError::TypeCheck { .. })));
        // Value pushed back.
        assert!(matches!(i.pop().unwrap(), PSValue::Integer(99)));
    }

    // ── op_def ────────────────────────────────────────────────────────────────

    #[test]
    fn def_binds_name_in_top_dict() {
        let mut i = Interpreter::new();
        i.push(PSValue::Name("x".into()));
        i.push(PSValue::Integer(42));
        op_def(&mut i).unwrap();
        assert!(matches!(i.dict_stack.lookup("x"), Some(PSValue::Integer(42))));
    }

    #[test]
    fn def_with_literal_name() {
        let mut i = Interpreter::new();
        // /foo 99 def  (Name variant for /foo)
        i.push(PSValue::Name("foo".into()));
        i.push(PSValue::Integer(99));
        op_def(&mut i).unwrap();
        assert!(matches!(i.dict_stack.lookup("foo"), Some(PSValue::Integer(99))));
    }

    #[test]
    fn def_with_executable_name() {
        let mut i = Interpreter::new();
        // foo 99 def  (ExecutableName variant)
        i.push(PSValue::ExecutableName("bar".into()));
        i.push(PSValue::Integer(7));
        op_def(&mut i).unwrap();
        assert!(matches!(i.dict_stack.lookup("bar"), Some(PSValue::Integer(7))));
    }

    #[test]
    fn def_non_name_key_errors() {
        let mut i = Interpreter::new();
        i.push(PSValue::Integer(1)); // invalid key
        i.push(PSValue::Integer(2));
        assert!(matches!(op_def(&mut i), Err(PSError::TypeCheck { .. })));
    }

    #[test]
    fn def_into_begin_pushed_dict() {
        let mut i = Interpreter::new();
        // Push a fresh dict, begin it, then def into it.
        i.push(PSValue::Dictionary(new_dict(4)));
        op_begin(&mut i).unwrap();
        i.push(PSValue::Name("y".into()));
        i.push(PSValue::Integer(100));
        op_def(&mut i).unwrap();
        assert!(matches!(i.dict_stack.lookup("y"), Some(PSValue::Integer(100))));
        // After end, "y" should no longer be visible.
        op_end(&mut i).unwrap();
        assert!(i.dict_stack.lookup("y").is_none());
    }

    // ── Dynamic lookup ────────────────────────────────────────────────────────

    #[test]
    fn dynamic_lookup_finds_top_binding() {
        let mut i = Interpreter::new();
        // Define x=10 in userdict.
        i.push(PSValue::Name("x".into()));
        i.push(PSValue::Integer(10));
        op_def(&mut i).unwrap();

        // Push a new dict that shadows x=20.
        i.push(PSValue::Dictionary(new_dict(4)));
        op_begin(&mut i).unwrap();
        i.push(PSValue::Name("x".into()));
        i.push(PSValue::Integer(20));
        op_def(&mut i).unwrap();

        // Dynamic lookup finds x=20 (top dict wins).
        assert!(matches!(i.dict_stack.lookup("x"), Some(PSValue::Integer(20))));

        op_end(&mut i).unwrap();

        // After end, x=10 is visible again.
        assert!(matches!(i.dict_stack.lookup("x"), Some(PSValue::Integer(10))));
    }

    // ── Lexical vs dynamic scoping demo ──────────────────────────────────────
    //
    // This test is the centrepiece of the scoping demonstration.
    //
    // Equivalent PostScript program:
    //
    //   % --- setup (same for both modes) ---
    //   /x 10 def           % bind x = 10 in userdict
    //
    //   % --- DYNAMIC mode ---
    //   %   foo's definition scope is NOT captured.
    //   %   At call time, a new dict with x=20 is on top of the stack.
    //   %   Lookup walks the CURRENT stack and finds x=20.
    //
    //   % --- LEXICAL mode ---
    //   %   At the moment foo is "defined" (proc literal evaluated),
    //   %   the interpreter captures a snapshot of the dict stack.
    //   %   The snapshot contains only [systemdict, userdict], where x=10.
    //   %   At call time, lookup searches the SNAPSHOT, not the live stack.
    //   %   The new dict with x=20 is NOT in the snapshot → x=10 is returned.
    //
    #[test]
    fn scoping_demo_dynamic_vs_lexical() {
        // ── Part 1: DYNAMIC SCOPING ───────────────────────────────────────────

        let mut dyn_interp = Interpreter::new();
        // use_lexical_scope defaults to false → dynamic mode.

        // /x 10 def  (in userdict / top of stack)
        dyn_interp.push(PSValue::Name("x".into()));
        dyn_interp.push(PSValue::Integer(10));
        op_def(&mut dyn_interp).unwrap();

        // Capture foo's "definition scope" — but in dynamic mode this does not
        // matter; it is included here only for symmetry with the lexical test.
        let _foo_definition_scope = dyn_interp.dict_stack.snapshot();

        // Simulate entering a local scope: `5 dict begin  /x 20 def`
        dyn_interp.push(PSValue::Dictionary(new_dict(4)));
        op_begin(&mut dyn_interp).unwrap();
        dyn_interp.push(PSValue::Name("x".into()));
        dyn_interp.push(PSValue::Integer(20));
        op_def(&mut dyn_interp).unwrap();

        // Dynamic lookup: walks CURRENT stack → [newdict(x=20), userdict(x=10), systemdict]
        // → finds x = 20 in newdict first.
        let dynamic_result = dyn_interp.dict_stack.lookup("x");
        assert!(
            matches!(dynamic_result, Some(PSValue::Integer(20))),
            "dynamic scoping should find the x=20 binding in the top dict"
        );

        // ── Part 2: LEXICAL SCOPING ───────────────────────────────────────────

        let mut lex_interp = Interpreter::new();
        lex_interp.use_lexical_scope = true;

        // /x 10 def  (in userdict)
        lex_interp.push(PSValue::Name("x".into()));
        lex_interp.push(PSValue::Integer(10));
        op_def(&mut lex_interp).unwrap();

        // Capture foo's scope HERE — at the moment the procedure would be
        // pushed onto the operand stack.  The dict stack currently contains
        // only [systemdict, userdict], so x=10 is the binding in scope.
        let foo_captured_scope = lex_interp.dict_stack.snapshot();
        // Confirm: x is visible in the snapshot with value 10.
        assert!(
            matches!(
                DictStack::lookup_in(&foo_captured_scope, "x"),
                Some(PSValue::Integer(10))
            ),
            "captured scope should contain x=10 at definition time"
        );

        // Now push a new dict that shadows x=20 — AFTER the snapshot was taken.
        lex_interp.push(PSValue::Dictionary(new_dict(4)));
        op_begin(&mut lex_interp).unwrap();
        lex_interp.push(PSValue::Name("x".into()));
        lex_interp.push(PSValue::Integer(20));
        op_def(&mut lex_interp).unwrap();

        // Live stack now has x=20 on top — same situation as dynamic mode.
        assert!(
            matches!(lex_interp.dict_stack.lookup("x"), Some(PSValue::Integer(20))),
            "live stack should see x=20 (sanity check)"
        );

        // LEXICAL lookup: searches foo's CAPTURED scope [systemdict, userdict].
        // The new dict with x=20 was pushed AFTER the snapshot → it is absent.
        // → x=10 is the result.
        let lexical_result = DictStack::lookup_in(&foo_captured_scope, "x");
        assert!(
            matches!(lexical_result, Some(PSValue::Integer(10))),
            "lexical scoping should find x=10 from the captured definition-time scope"
        );

        // ── Summary ───────────────────────────────────────────────────────────
        // Same program state, same name "x", but two different answers:
        //   dynamic  → 20   (sees the caller's current environment)
        //   lexical  → 10   (sees the environment at definition time)
        assert_ne!(
            match dynamic_result  { Some(PSValue::Integer(n)) => n, _ => -1 },
            match lexical_result  { Some(PSValue::Integer(n)) => n, _ => -1 },
            "dynamic and lexical scoping must produce different results for this program"
        );
    }

    // ── make_procedure captures scope in lexical mode ─────────────────────────

    #[test]
    fn make_procedure_no_capture_in_dynamic_mode() {
        let i = Interpreter::new(); // use_lexical_scope = false
        let body = Rc::new(vec![]);
        match i.make_procedure(body) {
            PSValue::Procedure(_, None) => {} // correct: no captured scope
            _ => panic!("dynamic mode must not attach a captured scope"),
        }
    }

    #[test]
    fn make_procedure_captures_scope_in_lexical_mode() {
        let mut i = Interpreter::new();
        i.use_lexical_scope = true;
        // Define x=10 so the snapshot is non-trivial.
        i.push(PSValue::Name("x".into()));
        i.push(PSValue::Integer(10));
        op_def(&mut i).unwrap();
        let body = Rc::new(vec![]);
        match i.make_procedure(body) {
            PSValue::Procedure(_, Some(scope)) => {
                // Captured scope must contain x=10.
                assert!(matches!(
                    DictStack::lookup_in(&scope, "x"),
                    Some(PSValue::Integer(10))
                ));
            }
            _ => panic!("lexical mode must attach a captured scope"),
        }
    }
}
