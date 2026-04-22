use std::rc::Rc;

use crate::lexer::{LexError, Lexer, Token};
use crate::types::{PSString, PSValue};

/// Errors the parser can produce.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("lex error: {0}")]
    Lex(#[from] LexError),
    #[error("unexpected `}}`: no matching `{{`")]
    UnmatchedRBrace,
    #[error("unexpected `]`: no matching `[`")]
    UnmatchedRBracket,
    #[error("unclosed procedure body `{{`")]
    UnclosedProcedure,
}

/// Parse a complete PostScript source string into a flat sequence of `PSValue`s.
///
/// Procedure bodies `{ ... }` are parsed recursively and returned as
/// `PSValue::Procedure`. Array literals `[ ... ]` are *not* fully parsed here
/// because they require the operand stack to construct — the parser emits the
/// raw bracket tokens as `PSValue::Mark` + a sentinel so the interpreter can
/// do the work. (This matches how most PS interpreters handle `[`/`]`.)
///
/// The returned `Vec<PSValue>` is the "program" the interpreter will execute
/// token by token.
pub fn parse(input: &str) -> Result<Vec<PSValue>, ParseError> {
    let mut lexer = Lexer::new(input);
    parse_sequence(&mut lexer, false)
}

/// Recursively parse tokens into a `Vec<PSValue>`.
///
/// `inside_proc` = `true` when we entered via a `{`; we stop on the matching `}`.
fn parse_sequence(lexer: &mut Lexer, inside_proc: bool) -> Result<Vec<PSValue>, ParseError> {
    let mut values: Vec<PSValue> = Vec::new();

    loop {
        let tok = match lexer.next_token()? {
            None => {
                if inside_proc {
                    return Err(ParseError::UnclosedProcedure);
                }
                break;
            }
            Some(t) => t,
        };

        match tok {
            Token::Integer(n) => values.push(PSValue::Integer(n)),
            Token::Float(v)   => values.push(PSValue::Float(v)),
            Token::Boolean(b) => values.push(PSValue::Boolean(b)),

            Token::PSString(bytes) => {
                values.push(PSValue::String(PSString::new(bytes)));
            }

            // `/foo` → a literal (non-executable) name pushed as a value.
            Token::LiteralName(name) => {
                values.push(PSValue::Name(Rc::from(name.as_str())));
            }

            // `foo` → an executable name; the interpreter will look it up.
            Token::ExecutableName(name) => {
                values.push(PSValue::ExecutableName(Rc::from(name.as_str())));
            }

            // `{` → recurse, collect body, wrap in Procedure.
            Token::LBrace => {
                let body = parse_sequence(lexer, true)?;
                // The parser has no access to the interpreter's scoping flag, so
                // all parsed procedures start with no captured scope (None).
                // The execution loop calls `interp.make_procedure(body)` instead
                // when it needs scope capture for lexical mode.
                values.push(PSValue::Procedure(Rc::new(body), None));
            }

            Token::RBrace => {
                if inside_proc {
                    break; // closing brace for this level
                }
                return Err(ParseError::UnmatchedRBrace);
            }

            // `[` and `]` are pushed as Mark / a special sentinel so the
            // interpreter can build the array on the operand stack at runtime.
            Token::LBracket  => values.push(PSValue::Mark),
            Token::RBracket  => values.push(PSValue::ExecutableName(Rc::from("]"))),
        }
    }

    Ok(values)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn exec_name(s: &str) -> PSValue { PSValue::ExecutableName(Rc::from(s)) }
    fn lit_name(s: &str)  -> PSValue { PSValue::Name(Rc::from(s)) }

    #[test]
    fn scalars() {
        let prog = parse("42 3.14 true false").unwrap();
        assert!(matches!(prog[0], PSValue::Integer(42)));
        assert!(matches!(prog[1], PSValue::Float(_)));
        assert!(matches!(prog[2], PSValue::Boolean(true)));
        assert!(matches!(prog[3], PSValue::Boolean(false)));
    }

    #[test]
    fn literal_and_executable_names() {
        let prog = parse("/foo add").unwrap();
        assert!(matches!(&prog[0], PSValue::Name(n) if n.as_ref() == "foo"));
        assert!(matches!(&prog[1], PSValue::ExecutableName(n) if n.as_ref() == "add"));
    }

    #[test]
    fn string_value() {
        let prog = parse("(hello)").unwrap();
        match &prog[0] {
            PSValue::String(s) => assert_eq!(s.to_bytes(), b"hello"),
            _ => panic!("expected String"),
        }
    }

    #[test]
    fn procedure_body() {
        let prog = parse("{ 1 add }").unwrap();
        match &prog[0] {
            PSValue::Procedure(body, scope) => {
                assert_eq!(body.len(), 2);
                assert!(matches!(body[0], PSValue::Integer(1)));
                assert!(matches!(&body[1], PSValue::ExecutableName(n) if n.as_ref() == "add"));
                // Parsed procedures always start with no captured scope.
                assert!(scope.is_none());
            }
            _ => panic!("expected Procedure"),
        }
    }

    #[test]
    fn nested_procedure() {
        let prog = parse("{ { 1 } exec }").unwrap();
        match &prog[0] {
            PSValue::Procedure(outer, _) => {
                assert!(matches!(outer[0], PSValue::Procedure(_, _)));
                assert!(matches!(&outer[1], PSValue::ExecutableName(n) if n.as_ref() == "exec"));
            }
            _ => panic!("expected Procedure"),
        }
    }

    #[test]
    fn unmatched_rbrace_is_error() {
        assert!(parse("}").is_err());
    }

    #[test]
    fn unclosed_proc_is_error() {
        assert!(parse("{ 1 add").is_err());
    }
}
