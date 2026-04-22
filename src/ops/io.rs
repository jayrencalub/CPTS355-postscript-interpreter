// ── I/O operators: print, =, == ───────────────────────────────────────────────
//
// These three operators all write to the interpreter's output stream
// (`Interpreter::output`), which defaults to stdout but can be swapped for any
// `Box<dyn Write>` — useful for capturing output in tests.
//
// ── The = vs == distinction ───────────────────────────────────────────────────
//
// Both `=` and `==` pop one value and write it followed by a newline.
// They differ in how they FORMAT the value:
//
//   `=`  — human-readable, like `cvs`.  Strips type-level delimiters:
//           • String  →  raw bytes, no surrounding `(` `)`.
//           • Literal name `/foo`  →  `foo` (no leading slash).
//           • Everything else is identical to `==`.
//
//   `==` — PostScript source syntax.  The output unambiguously encodes the TYPE:
//           • String `(hello)`  →  `(hello)` with parens.
//           • Literal name `/foo`  →  `/foo` with slash.
//           • Executable name `add`  →  `add` (no slash, same as `=`).
//           • Procedure `{ 1 add }`  →  `{ 1 add }` showing the body.
//           • Array `[1 2]`  →  `[1 2]`.
//
// Concrete example — the key difference for strings:
//
//   (hello) =    →  hello      ← just the characters, no delimiters
//   (hello) ==   →  (hello)    ← parens show it IS a string
//
// And for literal names:
//
//   /foo =    →  foo           ← stripped
//   /foo ==   →  /foo          ← slash preserved
//
// `print` is the lowest-level writer: strings only, no newline, no conversion.

use std::io::Write;

use crate::interpreter::Interpreter;
use crate::types::{PSError, PSValue};

// ── representation helpers ────────────────────────────────────────────────────

/// Full PostScript source syntax — used by `==`.
///
/// Recursively formats a `PSValue` so the result could (for scalars and simple
/// composites) be fed back to the parser to reconstruct the same value.
///
/// String escaping: characters that cannot appear literally inside `(...)` are
/// written as PostScript escape sequences:
///   `\`  →  `\\`
///   `(`  →  `\(`
///   `)`  →  `\)`
///   newline  →  `\n`
///   CR       →  `\r`
///   tab      →  `\t`
///   other control / high bytes  →  `\ddd` (3-digit octal)
pub fn ps_repr(val: &PSValue) -> String {
    match val {
        PSValue::Integer(n) => n.to_string(),
        PSValue::Float(f)   => format_float(*f),
        PSValue::Boolean(b) => b.to_string(),

        PSValue::String(s) => {
            let bytes = s.to_bytes();
            let mut out = String::with_capacity(bytes.len() + 2);
            out.push('(');
            for &byte in &bytes {
                match byte {
                    b'\\' => out.push_str("\\\\"),
                    b'('  => out.push_str("\\("),
                    b')'  => out.push_str("\\)"),
                    b'\n' => out.push_str("\\n"),
                    b'\r' => out.push_str("\\r"),
                    b'\t' => out.push_str("\\t"),
                    // Control characters and high bytes → octal escape.
                    0x00..=0x1f | 0x7f..=0xff => {
                        out.push_str(&format!("\\{byte:03o}"));
                    }
                    _ => out.push(byte as char),
                }
            }
            out.push(')');
            out
        }

        // Literal name includes the leading slash.
        PSValue::Name(n) => format!("/{n}"),
        // Executable name has no slash.
        PSValue::ExecutableName(n) => n.to_string(),

        PSValue::Array(a) => {
            let items = a.borrow();
            if items.is_empty() {
                "[]".to_string()
            } else {
                let parts: Vec<String> = items.iter().map(ps_repr).collect();
                format!("[{}]", parts.join(" "))
            }
        }

        PSValue::Procedure(body, _) => {
            if body.is_empty() {
                "{}".to_string()
            } else {
                let parts: Vec<String> = body.iter().map(ps_repr).collect();
                format!("{{ {} }}", parts.join(" "))
            }
        }

        PSValue::Dictionary(_) => "-dict-".to_string(),
        PSValue::Null          => "null".to_string(),
        PSValue::Operator(_)   => "--operator--".to_string(),
        PSValue::Mark          => "--mark--".to_string(),
    }
}

/// Human-readable form — used by `=`.
///
/// For strings: the raw byte content, no `(` `)` delimiters.
/// For literal names: the name characters, no leading `/`.
/// For everything else: same as `ps_repr`.
fn ps_cvs(val: &PSValue) -> String {
    match val {
        PSValue::String(s) => String::from_utf8_lossy(&s.to_bytes()).into_owned(),
        PSValue::Name(n)   => n.to_string(),
        other              => ps_repr(other),
    }
}

/// Format a float so the output always contains a decimal point or exponent,
/// making it unambiguously a real number rather than an integer.
///
/// Rust's default `{}` formatter prints `3.0` as `"3"`, which PostScript would
/// re-parse as an integer.  This function appends `.0` when needed.
fn format_float(f: f64) -> String {
    let s = format!("{f}");
    if s.contains('.') || s.contains('e') || s.contains('E')
        || s.contains('n')   // nan
        || s.contains('i')   // inf
    {
        s
    } else {
        format!("{s}.0")
    }
}

// ── operators ─────────────────────────────────────────────────────────────────

/// `print` — write a string's raw bytes to the output stream, no newline.
///
/// Stack effect: `string → `
///
/// The lowest-level output operator.  Accepts only strings; does not perform
/// any conversion and does not append a newline.  Binary bytes stored in the
/// string are written as-is.
///
/// Errors: `typecheck` if the top is not a string (value is pushed back).
pub fn op_print(interp: &mut Interpreter) -> Result<(), PSError> {
    match interp.pop()? {
        PSValue::String(s) => {
            interp.output
                .write_all(&s.to_bytes())
                .map_err(|e| PSError::Other(e.to_string()))
        }
        other => {
            interp.push(other);
            Err(PSError::TypeCheck { expected: "string", got: "non-string" })
        }
    }
}

/// `=` — write the human-readable form of any value, followed by a newline.
///
/// Stack effect: `any → `
///
/// Equivalent in intent to PostScript's `cvs print (\n) print`: converts the
/// value to its string representation, writes it, then writes a newline.
///
/// The key property of `cvs` is that applying it to a string returns the string
/// ITSELF — so printing the result writes the raw bytes without any surrounding
/// delimiters.  Likewise a literal name `/foo` becomes `foo` (just the
/// characters).
///
/// For numbers, booleans, arrays, procedures, dictionaries, and null the output
/// is identical to `==`.
pub fn op_equal(interp: &mut Interpreter) -> Result<(), PSError> {
    let val = interp.pop()?;
    let text = ps_cvs(&val);
    writeln!(interp.output, "{text}").map_err(|e| PSError::Other(e.to_string()))
}

/// `==` — write the full PostScript source representation, followed by a newline.
///
/// Stack effect: `any → `
///
/// Unlike `=`, the output encodes the TYPE of the value as well as its content:
///
/// | Value pushed | `=` output | `==` output |
/// |---|---|---|
/// | `(hello)` string | `hello` | `(hello)` |
/// | `/foo` literal name | `foo` | `/foo` |
/// | `add` executable name | `add` | `add` |
/// | integer `42` | `42` | `42` |
/// | real `3.0` | `3.0` | `3.0` |
/// | `true` / `false` | `true` / `false` | `true` / `false` |
/// | array `[1 2]` | `[1 2]` | `[1 2]` |
/// | procedure `{ add }` | `{ add }` | `{ add }` |
///
/// Special characters inside strings are written as PostScript escape sequences
/// so the output can be re-parsed correctly.
pub fn op_equal_equal(interp: &mut Interpreter) -> Result<(), PSError> {
    let val = interp.pop()?;
    let text = ps_repr(&val);
    writeln!(interp.output, "{text}").map_err(|e| PSError::Other(e.to_string()))
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PSString;
    use std::cell::RefCell;
    use std::rc::Rc;

    // ── test infrastructure ───────────────────────────────────────────────────
    //
    // `make_interp` constructs an Interpreter whose output goes into a shared
    // Rc<RefCell<Vec<u8>>>, so we can inspect what was written after each call
    // without spawning a subprocess or redirecting stdout.

    struct SharedBuf(Rc<RefCell<Vec<u8>>>);

    impl Write for SharedBuf {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn make_interp() -> (Interpreter, Rc<RefCell<Vec<u8>>>) {
        let buf = Rc::new(RefCell::new(Vec::<u8>::new()));
        let writer = SharedBuf(Rc::clone(&buf));
        let interp = Interpreter::with_output(Box::new(writer));
        (interp, buf)
    }

    fn captured(buf: &Rc<RefCell<Vec<u8>>>) -> String {
        String::from_utf8(buf.borrow().clone()).expect("output should be valid UTF-8")
    }

    // value constructors
    fn str_val(s: &[u8]) -> PSValue { PSValue::String(PSString::new(s.to_vec())) }
    fn int(n: i64)        -> PSValue { PSValue::Integer(n) }
    fn flt(f: f64)        -> PSValue { PSValue::Float(f) }
    fn bool_(b: bool)     -> PSValue { PSValue::Boolean(b) }
    fn name(s: &str)      -> PSValue { PSValue::Name(Rc::from(s)) }
    fn xname(s: &str)     -> PSValue { PSValue::ExecutableName(Rc::from(s)) }

    // ── print ─────────────────────────────────────────────────────────────────

    #[test]
    fn print_writes_raw_bytes_no_newline() {
        let (mut i, buf) = make_interp();
        i.push(str_val(b"hello"));
        op_print(&mut i).unwrap();
        assert_eq!(captured(&buf), "hello"); // no trailing newline
    }

    #[test]
    fn print_empty_string() {
        let (mut i, buf) = make_interp();
        i.push(str_val(b""));
        op_print(&mut i).unwrap();
        assert_eq!(captured(&buf), "");
    }

    #[test]
    fn print_sequential_calls_concatenate() {
        let (mut i, buf) = make_interp();
        i.push(str_val(b"foo"));
        op_print(&mut i).unwrap();
        i.push(str_val(b"bar"));
        op_print(&mut i).unwrap();
        assert_eq!(captured(&buf), "foobar");
    }

    #[test]
    fn print_non_string_typecheck_restores_stack() {
        let (mut i, _) = make_interp();
        i.push(int(42));
        assert!(matches!(op_print(&mut i), Err(PSError::TypeCheck { .. })));
        assert!(matches!(i.pop().unwrap(), PSValue::Integer(42)));
    }

    // ── = ─────────────────────────────────────────────────────────────────────

    #[test]
    fn equal_string_prints_raw_bytes_with_newline() {
        let (mut i, buf) = make_interp();
        i.push(str_val(b"hello"));
        op_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "hello\n");
    }

    #[test]
    fn equal_integer() {
        let (mut i, buf) = make_interp();
        i.push(int(42));
        op_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "42\n");
    }

    #[test]
    fn equal_negative_integer() {
        let (mut i, buf) = make_interp();
        i.push(int(-7));
        op_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "-7\n");
    }

    #[test]
    fn equal_float_with_fraction() {
        let (mut i, buf) = make_interp();
        i.push(flt(3.14));
        op_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "3.14\n");
    }

    #[test]
    fn equal_float_whole_number_has_decimal_point() {
        let (mut i, buf) = make_interp();
        i.push(flt(3.0));
        op_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "3.0\n");
    }

    #[test]
    fn equal_bool_true() {
        let (mut i, buf) = make_interp();
        i.push(bool_(true));
        op_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "true\n");
    }

    #[test]
    fn equal_bool_false() {
        let (mut i, buf) = make_interp();
        i.push(bool_(false));
        op_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "false\n");
    }

    #[test]
    fn equal_literal_name_strips_slash() {
        // `=` on a literal name prints the characters, no leading `/`.
        let (mut i, buf) = make_interp();
        i.push(name("foo"));
        op_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "foo\n");
    }

    #[test]
    fn equal_executable_name() {
        let (mut i, buf) = make_interp();
        i.push(xname("add"));
        op_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "add\n");
    }

    // ── == ────────────────────────────────────────────────────────────────────

    #[test]
    fn equal_equal_string_wraps_in_parens() {
        let (mut i, buf) = make_interp();
        i.push(str_val(b"hello"));
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "(hello)\n");
    }

    #[test]
    fn equal_equal_integer() {
        let (mut i, buf) = make_interp();
        i.push(int(42));
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "42\n");
    }

    #[test]
    fn equal_equal_float_whole_has_decimal_point() {
        let (mut i, buf) = make_interp();
        i.push(flt(3.0));
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "3.0\n");
    }

    #[test]
    fn equal_equal_bool_false() {
        let (mut i, buf) = make_interp();
        i.push(bool_(false));
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "false\n");
    }

    #[test]
    fn equal_equal_literal_name_keeps_slash() {
        // `==` on a literal name preserves the `/` so the type is unambiguous.
        let (mut i, buf) = make_interp();
        i.push(name("foo"));
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "/foo\n");
    }

    #[test]
    fn equal_equal_executable_name_no_slash() {
        let (mut i, buf) = make_interp();
        i.push(xname("add"));
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "add\n");
    }

    #[test]
    fn equal_equal_string_escapes_backslash() {
        let (mut i, buf) = make_interp();
        i.push(str_val(b"a\\b"));
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "(a\\\\b)\n");
    }

    #[test]
    fn equal_equal_string_escapes_parens() {
        let (mut i, buf) = make_interp();
        i.push(str_val(b"(hi)"));
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "(\\(hi\\))\n");
    }

    #[test]
    fn equal_equal_string_escapes_newline() {
        let (mut i, buf) = make_interp();
        i.push(str_val(b"line1\nline2"));
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "(line1\\nline2)\n");
    }

    #[test]
    fn equal_equal_string_escapes_control_byte_as_octal() {
        // ASCII BEL (0x07) has no named escape in PostScript, so it uses \007.
        let (mut i, buf) = make_interp();
        i.push(str_val(&[b'x', 0x07, b'y']));
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "(x\\007y)\n");
    }

    #[test]
    fn equal_equal_array() {
        let (mut i, buf) = make_interp();
        i.push(PSValue::Array(Rc::new(RefCell::new(vec![int(1), int(2), int(3)]))));
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "[1 2 3]\n");
    }

    #[test]
    fn equal_equal_empty_array() {
        let (mut i, buf) = make_interp();
        i.push(PSValue::Array(Rc::new(RefCell::new(vec![]))));
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "[]\n");
    }

    #[test]
    fn equal_equal_procedure_shows_body() {
        // Procedure body is rendered as `{ token token ... }`.
        let body = vec![int(1), xname("add")];
        let (mut i, buf) = make_interp();
        i.push(PSValue::Procedure(Rc::new(body), None));
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "{ 1 add }\n");
    }

    #[test]
    fn equal_equal_empty_procedure() {
        let (mut i, buf) = make_interp();
        i.push(PSValue::Procedure(Rc::new(vec![]), None));
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "{}\n");
    }

    #[test]
    fn equal_equal_null() {
        let (mut i, buf) = make_interp();
        i.push(PSValue::Null);
        op_equal_equal(&mut i).unwrap();
        assert_eq!(captured(&buf), "null\n");
    }

    // ── the key difference ────────────────────────────────────────────────────

    #[test]
    fn string_equal_vs_equal_equal_differ() {
        // `=` strips delimiters; `==` wraps in parens.
        let (mut i1, buf1) = make_interp();
        i1.push(str_val(b"world"));
        op_equal(&mut i1).unwrap();

        let (mut i2, buf2) = make_interp();
        i2.push(str_val(b"world"));
        op_equal_equal(&mut i2).unwrap();

        assert_eq!(captured(&buf1), "world\n");
        assert_eq!(captured(&buf2), "(world)\n");
        assert_ne!(captured(&buf1), captured(&buf2));
    }

    #[test]
    fn name_equal_vs_equal_equal_differ() {
        // `=` strips the slash; `==` keeps it.
        let (mut i1, buf1) = make_interp();
        i1.push(name("myname"));
        op_equal(&mut i1).unwrap();

        let (mut i2, buf2) = make_interp();
        i2.push(name("myname"));
        op_equal_equal(&mut i2).unwrap();

        assert_eq!(captured(&buf1), "myname\n");
        assert_eq!(captured(&buf2), "/myname\n");
    }

    #[test]
    fn numbers_equal_and_equal_equal_are_identical() {
        // For numbers the two operators produce the same output.
        let (mut i1, buf1) = make_interp();
        i1.push(int(99));
        op_equal(&mut i1).unwrap();

        let (mut i2, buf2) = make_interp();
        i2.push(int(99));
        op_equal_equal(&mut i2).unwrap();

        assert_eq!(captured(&buf1), captured(&buf2));
    }
}
