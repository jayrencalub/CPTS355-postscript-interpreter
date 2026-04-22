# Rust Experience Report — Talking Points

---

## Language Characteristics

### Compiled or Interpreted?
- Rust is **compiled ahead-of-time** via the LLVM backend, producing a native binary.
- Unlike Python or Java, there is no runtime interpreter or virtual machine.
  Running `cargo build` produces a single self-contained executable — no JVM, no `.pyc`, no runtime dependency.
- Contrast with C#/Java: those compile to bytecode that runs on a managed runtime (CLR/JVM).
  Rust compiles directly to machine code, similar to C/C++, but with safety guarantees enforced at compile time rather than runtime.

### Static or Dynamic Typing?
- Rust uses **static typing** with **type inference**.
- You rarely write types explicitly on local variables — the compiler infers them — but the types are fully resolved at compile time, not at runtime.
- Contrast: Python uses dynamic typing (types are checked at runtime and can change).
  Java/C# use static typing but require explicit type annotations in most places.
  Rust sits between the two ergonomically: you get type safety without the verbosity.
- Example from this project: `PSValue` is a strongly-typed enum. The compiler enforces exhaustive matching — you cannot forget to handle a variant.

### Programming Paradigm
- Rust is **multi-paradigm**, supporting imperative, functional, and systems-level programming.
- It does **not** have classes or inheritance (no OOP in the traditional sense).
  It uses `struct` + `impl` for data with methods, and `trait` for shared behavior — closer to Go's interfaces or Haskell's typeclasses than to Java's class hierarchy.
- The paradigm Rust **favors**: a **systems/functional hybrid**.
  Iterators, closures, pattern matching with `match`, and `Result`/`Option` chaining are idiomatic Rust and are borrowed from functional languages.
  The memory model (ownership, borrowing) is unique to Rust — it has no direct equivalent in C++, Java, Python, or C#.

---

## Language Selection

### Why Rust for this project?
- **Memory safety without a garbage collector.** PostScript has reference semantics for compound objects (strings, arrays, dicts). In C++ this would mean raw pointers, smart pointers, or manual lifetime tracking. In Java/C#/Python a GC handles it transparently. Rust's ownership system lets you express reference semantics explicitly without a GC and without risking dangling pointers or double-frees.
- **The `enum` type maps directly onto PostScript values.** Every PostScript value is one of: integer, float, boolean, string, name, array, dictionary, procedure, operator, or null. Rust's enums with associated data model this exactly:
  ```rust
  pub enum PSValue {
      Integer(i64),
      Float(f64),
      Boolean(bool),
      String(PSString),
      Name(Rc<str>),
      Array(Rc<RefCell<Vec<PSValue>>>),
      Dictionary(DictRef),
      Procedure(Rc<Vec<PSValue>>, Option<Rc<Vec<DictRef>>>),
      Operator(Rc<dyn Fn(&mut Interpreter) -> Result<(), PSError>>),
      Null,
      Mark,
  }
  ```
  In C++ this would require a tagged union or `std::variant`, which is more verbose and less safe. In Java/Python you would use polymorphism or duck typing, losing the compile-time guarantee that all cases are handled.
- **`cargo` toolchain.** Testing, building, and dependency management all work out of the box with one tool. No Makefile, no CMake, no manually linking libraries.
- **Pattern matching.** Every operator function `match`es on the stack values it pops. The compiler enforces that every variant is accounted for, catching bugs at compile time that would only surface at runtime in Python or Java.

---

## Positive Discovery

### `Rc<RefCell<T>>` for PostScript's reference semantics

**What it is:** `Rc<T>` gives shared ownership (multiple owners, reference-counted). `RefCell<T>` gives interior mutability — the ability to mutate data through a shared reference, with borrow rules checked at runtime instead of compile time.

**Why it was impressive:** PostScript specifies that `getinterval` returns a *substring alias* — a new string object that shares the same underlying bytes as the original. A `putinterval` through the alias mutates bytes visible through the original. This is reference semantics that is trivial in Python or Java (everything is already a reference) and very manual in C++ (you manage the pointer yourself). In Rust, `Rc<RefCell<Vec<u8>>>` gives you exactly this — shared ownership plus mutability — with the compiler tracking when it is safe.

```rust
pub struct PSString {
    pub buf:    Rc<RefCell<Vec<u8>>>,  // shared backing buffer
    pub offset: usize,                 // where this view starts
    pub length: usize,                 // how many bytes this view exposes
}

impl PSString {
    // getinterval: returns a new PSString that shares buf with self.
    // No bytes are copied — both objects alias the same Vec<u8>.
    pub fn get_interval(&self, start: usize, count: usize) -> Option<Self> {
        if start + count <= self.length {
            Some(Self {
                buf: Rc::clone(&self.buf),       // clone the pointer, not the data
                offset: self.offset + start,
                length: count,
            })
        } else {
            None
        }
    }
}
```

**Why it stood out:** In C++ you would manage raw pointers or `shared_ptr` manually and risk use-after-free. In Java/Python you get this for free but have no way to reason about *when* mutation happens. Rust's approach makes the aliasing *explicit and visible in the type*: seeing `Rc<RefCell<...>>` in a field tells you immediately "this is shared and mutable." The tradeoff — runtime borrow checks via `RefCell` instead of compile-time checks — is documented clearly and the compiler reminds you when you violate it.

**Contrast with C++:** `std::shared_ptr<std::vector<uint8_t>>` gives shared ownership but no interior mutability protection. Two threads (or two calls) can mutate through the same `shared_ptr` with no enforcement. `RefCell` makes the borrow rules explicit even in a single-threaded context.

---

## Challenges and Frustrations

### 1. The borrow checker blocks lookup-then-mutate patterns

**The scenario:** The interpreter's execution loop needs to look up a name in the dictionary stack, then call the resulting operator (which requires mutable access to the interpreter). In most languages, this would be written naturally:

```rust
// What you'd write in pseudocode / Java / Python:
let val = self.dict_stack.lookup(name); // borrows self
val.call(self);                          // mutates self — compiler error!
```

Rust rejects this because `lookup` holds an immutable borrow of `self` through its return value, and `call(self)` requires a mutable borrow — the two overlap.

**The fix required:** `lookup` must return an *owned* clone of the value so the borrow ends before the mutable call:

```rust
// What Rust actually requires:
pub fn lookup(&self, key: &str) -> Option<PSValue> {
    for dict in self.frames.iter().rev() {
        if let Some(val) = dict.borrow().get(key) {
            return Some(val.clone());   // owned copy — borrow ends here
        }
    }
    None
}

// Now the call works:
let found = self.lookup_name(name, exec_scope);  // borrow ends
match found {
    Some(PSValue::Operator(f)) => f(self)?,       // mutable borrow — no conflict
    ...
}
```

**Why it's frustrating:** In C++, Java, or Python you would never think twice about this. The pattern of "look something up then act on it" is so common that it is invisible. In Rust, you have to understand *why* the compiler objects and restructure the code around ownership. The fix is clean once you understand it, but the error message alone does not tell you what to do — it just says the borrows conflict.

---

### 2. `dyn Fn` in an enum breaks derived traits

**The scenario:** Built-in operators are stored as closures inside `PSValue`:

```rust
Operator(Rc<dyn Fn(&mut Interpreter) -> Result<(), PSError>>),
```

`dyn Fn` does not implement `Debug`, `Clone` (via `derive`), or `PartialEq`. This means `#[derive(Debug)]` fails on the entire enum, even though every other variant would derive it just fine.

**The fix required:** A manual `Debug` impl just for the `Operator` variant:

```rust
impl fmt::Debug for PSValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PSValue::Operator(_) => write!(f, "Operator(..)"),
            other => write!(f, "{other}"),
        }
    }
}
```

And `PartialEq` cannot be derived at all, so every test that needs to check a value uses `matches!` instead of `assert_eq!`:

```rust
// Cannot do: assert_eq!(val, PSValue::Integer(7));
// Must do:
assert!(matches!(val, PSValue::Integer(7)));
```

**Why it's frustrating:** In Java or C#, objects implement `equals()` and `toString()` through inheritance — the base class provides a default. In Python, `__eq__` and `__repr__` can be defined at any granularity. Rust's derive macros are all-or-nothing per trait: if a single field doesn't implement the trait, the entire derive fails. The manual workaround is not difficult but adds boilerplate that would not exist in any of the other languages.

---

### 3. `RefCell` runtime borrow panics require careful design

**The scenario:** `putinterval` needs to read bytes from the source string and write them into the destination. When source and destination alias the same `Rc`, they share the same `RefCell`. Trying to hold both a `borrow()` and a `borrow_mut()` at the same time through the same `RefCell` panics at runtime:

```rust
// This panics at runtime if src and dest share the same Rc:
let src_bytes = src.buf.borrow();          // immutable borrow
let mut dest  = self.buf.borrow_mut();     // mutable borrow — PANIC
```

**The fix required:** Eagerly copy the source bytes into a `Vec` before acquiring the mutable borrow, so no two borrows are live simultaneously:

```rust
// Phase 1: collect source bytes; immutable borrow dropped when block ends.
let src_bytes: Vec<u8> = {
    let b = src.buf.borrow();
    b[src.offset..src.offset + src.length].to_vec()
};
// Phase 2: mutate; RefCell has no active borrows now.
let mut b = self.buf.borrow_mut();
for (i, &byte) in src_bytes.iter().enumerate() {
    b[self.offset + start + i] = byte;
}
```

**Why it's frustrating:** `RefCell` defers the borrow check to runtime, which means a bug that in safe Rust *should* be caught at compile time instead panics in production if you are not careful. C++ has no borrow rules so this scenario simply compiles and runs (possibly incorrectly). Java/Python also have no such constraint. The issue is specific to Rust's interior-mutability model: you pay for the safety guarantee with a runtime cost, and you have to consciously design around it. The error is a panic with a message like "already borrowed: BorrowMutError," which does not immediately point to the root cause.

---

### Summary of differences from other languages

| Concern | C++ | Java/C# | Python | Rust |
|---|---|---|---|---|
| Reference semantics for mutable data | `shared_ptr` (no borrow rules) | GC handles it | GC handles it | `Rc<RefCell<T>>` — explicit, safe |
| Enum with associated data | `std::variant` (verbose) | polymorphism/sealed classes | no equivalent | first-class, exhaustively checked |
| Look-up-then-mutate pattern | works freely | works freely | works freely | requires owned return from lookup |
| Traits on closure types | no equivalent | no equivalent | no equivalent | `dyn Fn` blocks derive macros |
| Runtime borrow violations | undefined behavior / crash | no concept | no concept | explicit panic with `RefCell` |
