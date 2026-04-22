# PostScript Interpreter

A PostScript interpreter written in Rust for CPTS 355.

---

## Building and Running

**Prerequisites:** Rust toolchain (edition 2024, Rust 1.85+).

```sh
# Build
cargo build

# Run the binary (scaffold entry point — no REPL yet)
cargo run

# Run the full test suite (390 tests)
cargo test

# Run a specific test module
cargo test ops::control
cargo test comprehensive::integration
```

The interpreter library is fully exercised through `cargo test`. The `main` binary is a placeholder while the interactive frontend is pending.

### Using the interpreter programmatically

```rust
use postscript_interpreter::{interpreter::Interpreter, parser::parse};

let program = parse("3 4 add 2 mul").unwrap();
let mut interp = Interpreter::new();
interp.exec_body(&program, None).unwrap();
// operand stack now holds [Integer(14)]
```

To capture `print` / `=` / `==` output (as the test suite does):

```rust
use std::{cell::RefCell, io::Write, rc::Rc};

struct Buf(Rc<RefCell<Vec<u8>>>);
impl Write for Buf {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

let sink = Rc::new(RefCell::new(Vec::<u8>::new()));
let mut interp = Interpreter::with_output(Box::new(Buf(Rc::clone(&sink))));
// run your program, then read sink.borrow() for the output bytes
```

---

## Dynamic vs. Lexical Scoping

PostScript originally specifies **dynamic scoping**: name lookup walks the live dictionary stack at call time. This interpreter also supports **lexical scoping** via a runtime flag.

### Toggling the mode

```rust
let mut interp = Interpreter::new();        // dynamic by default
interp.use_lexical_scope = true;            // switch to lexical
```

### What changes

| | Dynamic | Lexical |
|---|---|---|
| Name lookup | searches the **current** dict stack when the procedure is **called** | searches the dict stack snapshot taken when the procedure was **defined** |
| `begin`-pushed dicts | visible to all subsequent calls | only visible to code defined **after** `begin` |
| Performance | no snapshot overhead | one `Rc::clone` per dict-stack frame at definition time |

### Concrete example

```postscript
/x 10 def
/foo { x } def      % snapshot taken here (lexical) → [systemdict, userdict{x=10}]

5 dict begin
  /x 20 def         % userdict still has x=10; this new dict has x=20

  foo               % dynamic → finds x=20 (newdict is on the live stack)
                    % lexical  → finds x=10 (newdict was pushed after the snapshot)
end
```

Under dynamic scoping `foo` pushes `20`; under lexical scoping it pushes `10`.

### How the mechanism works

**Scope capture — `Interpreter::make_procedure`**

When `use_lexical_scope = true`, every procedure literal evaluated during execution is wrapped with a snapshot of the current dict stack:

```rust
// src/interpreter.rs
pub fn make_procedure(&self, body: Rc<Vec<PSValue>>) -> PSValue {
    let captured = if self.use_lexical_scope {
        Some(Rc::new(self.dict_stack.snapshot()))  // cheap: clones Rc pointers, not HashMaps
    } else {
        None
    };
    PSValue::Procedure(body, captured)
}
```

`DictStack::snapshot()` clones the `Rc` pointers in the stack, not the HashMap contents. The snapshot is a lightweight list of shared pointers — mutations to an already-captured dict (e.g. a later `def x 30` in userdict) **are** visible through the snapshot because the same `Rc` is shared. What the snapshot prevents is newly `begin`-pushed dicts from being visible, since they were not in the list at capture time.

**Name lookup — `Interpreter::lookup_name`**

```rust
pub fn lookup_name(&self, key: &str, exec_scope: Option<&[DictRef]>) -> Option<PSValue> {
    if self.use_lexical_scope {
        if let Some(scope) = exec_scope {
            return Self::lookup_lexical(scope, key)
                .or_else(|| self.dict_stack.lookup(key)); // systemdict fallback
        }
    }
    self.dict_stack.lookup(key)
}
```

In lexical mode the executing procedure's captured scope is searched first, then the live stack as a fallback so built-in operators in `systemdict` are always reachable.

---

## Rust Ownership Challenges

Three commands required non-obvious implementation strategies because of Rust's ownership and borrowing rules.

---

### 1. `putinterval` — aliased `RefCell` double-borrow

**The problem.**  
PostScript strings use `Rc<RefCell<Vec<u8>>>` so that `getinterval` can return a substring that shares the same backing buffer as the original (aliasing semantics from the PLRM). When the source and destination of a `putinterval` point to the same `Rc`, a naive implementation holds an immutable `borrow()` of the source and a mutable `borrow_mut()` of the destination *at the same time through the same `RefCell`*. `RefCell` enforces at runtime what the borrow checker enforces at compile time: you cannot hold both simultaneously — it panics.

**The fix.**  
Read all source bytes into a temporary `Vec<u8>` and drop the immutable borrow before acquiring the mutable one:

```rust
// src/types.rs — PSString::put_interval
pub fn put_interval(&self, start: usize, src: &PSString) -> bool {
    if start + src.length > self.length { return false; }

    // Phase 1 — read: immutable borrow ends when this block exits.
    let src_bytes: Vec<u8> = {
        let b = src.buf.borrow();
        b[src.offset..src.offset + src.length].to_vec()
    };

    // Phase 2 — write: no active borrows remain on the RefCell.
    let dest_base = self.offset + start;
    let mut b = self.buf.borrow_mut();
    for (i, &byte) in src_bytes.iter().enumerate() {
        b[dest_base + i] = byte;
    }
    true
}
```

This is correct whether or not source and destination alias, at the cost of one heap allocation per `putinterval` call.

---

### 2. `exec_body` / operators — borrow conflict between lookup and mutation

**The problem.**  
The execution loop calls `lookup_name`, which borrows `self` immutably (to read the dict stack), and then immediately calls `f(self)` or recurses into `exec_body`, both of which borrow `self` mutably. The compiler rejects this overlap even though the two borrows are logically sequential, because the lifetime of the first borrow would otherwise extend through the second call.

**The fix.**  
`lookup_name` returns an **owned** `PSValue` — the value is cloned out of the HashMap before the function returns:

```rust
// src/dict_stack.rs — DictStack::lookup
pub fn lookup(&self, key: &str) -> Option<PSValue> {
    for dict in self.frames.iter().rev() {
        if let Some(val) = dict.borrow().get(key) {
            return Some(val.clone());   // ← owned copy; immutable borrow ends here
        }
    }
    None
}
```

```rust
// src/interpreter.rs — exec_body (simplified)
let found = self.lookup_name(name, exec_scope);  // immutable borrow ENDS here
match found {
    Some(PSValue::Operator(f)) => f(self)?,       // mutable borrow — no conflict
    Some(PSValue::Procedure(..)) => self.exec_body(..)?,
    ...
}
```

Cloning `PSValue` is cheap for most variants: `Integer`, `Float`, `Boolean` copy inline; `String`, `Array`, `Dictionary`, `Procedure`, `Operator` clone an `Rc` pointer (one word). Deep copies are never made.

---

### 3. `Operator` variant — `dyn Fn` inside an enum

**The problem.**  
Built-in operators are stored as `Rc<dyn Fn(&mut Interpreter) -> Result<(), PSError>>`. `dyn Fn` doesn't implement `Debug`, `Clone` (via derive), or `PartialEq`, which blocks deriving those traits on the entire `PSValue` enum.

**The fix — `Debug`.**  
A manual `fmt::Debug` impl is provided that formats the `Operator` variant as the placeholder string `"Operator(..)"`:

```rust
impl fmt::Debug for PSValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PSValue::Operator(_) => write!(f, "Operator(..)"),
            other => write!(f, "{other}"),  // delegates to Display for everything else
        }
    }
}
```

**`PartialEq` is intentionally not implemented.** Comparing two `dyn Fn` values for equality is not possible in Rust (there is no function pointer equality for closures). Tests that need to inspect a value's type use `matches!` instead of `assert_eq!`:

```rust
// Instead of: assert_eq!(val, PSValue::Integer(7));
assert!(matches!(val, PSValue::Integer(7)));
```

**`Clone`** works because `Rc::clone` is derived automatically and cloning an `Rc<dyn Fn>` just increments the reference count without copying the closure body.

---

## Implemented Operators

| Category | Operators |
|---|---|
| Arithmetic | `add` `sub` `mul` `div` `idiv` `mod` `abs` `neg` `ceiling` `floor` `round` `sqrt` |
| Comparison | `eq` `ne` `lt` `le` `gt` `ge` |
| Logical | `and` `or` `not` `true` `false` |
| Stack | `dup` `exch` `pop` `copy` `clear` `count` |
| Dictionary | `dict` `length` `maxlength` `begin` `end` `def` |
| String | `length` `get` `getinterval` `putinterval` |
| Control flow | `if` `ifelse` `repeat` `for` `quit` |
| I/O | `print` `=` `==` |

See [IMPLEMENTATION.md](IMPLEMENTATION.md) for design decisions, operator semantics, and test coverage details for each category.
