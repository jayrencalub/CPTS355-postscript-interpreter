/// A raw token produced by the lexer before any semantic interpretation.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    /// A bare integer literal, e.g. `42` or `-7`.
    Integer(i64),
    /// A floating-point literal, e.g. `3.14` or `-.5e2`.
    Float(f64),
    /// `true` or `false`.
    Boolean(bool),
    /// A literal name, e.g. `/foo` (the leading `/` is consumed).
    LiteralName(String),
    /// An executable name, e.g. `add`, `dup`, `myproc`.
    ExecutableName(String),
    /// A string delimited by parentheses, e.g. `(hello)`. Escape sequences resolved.
    PSString(Vec<u8>),
    /// Opening brace `{` — signals the start of a procedure body.
    LBrace,
    /// Closing brace `}`.
    RBrace,
    /// `[` — array mark.
    LBracket,
    /// `]` — array close.
    RBracket,
}

/// Lexer state: wraps a `&str` and advances through it producing `Token`s.
pub struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { src: input.as_bytes(), pos: 0 }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let b = self.src.get(self.pos).copied();
        if b.is_some() { self.pos += 1; }
        b
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(b) if b.is_ascii_whitespace() => { self.advance(); }
                // `%` starts a line comment; skip to end of line.
                Some(b'%') => {
                    while matches!(self.peek(), Some(b) if b != b'\n') {
                        self.advance();
                    }
                }
                _ => break,
            }
        }
    }

    /// Collect bytes while predicate holds.
    fn take_while(&mut self, pred: impl Fn(u8) -> bool) -> Vec<u8> {
        let mut buf = Vec::new();
        while let Some(b) = self.peek() {
            if pred(b) { buf.push(b); self.advance(); } else { break; }
        }
        buf
    }

    /// Lex a PS string literal `(...)`. Handles nested parens and escape sequences.
    fn lex_string(&mut self) -> Result<Vec<u8>, LexError> {
        // Opening `(` already consumed by caller.
        let mut buf = Vec::new();
        let mut depth = 1usize;
        loop {
            match self.advance() {
                None => return Err(LexError::UnterminatedString),
                Some(b')') => {
                    depth -= 1;
                    if depth == 0 { break; }
                    buf.push(b')');
                }
                Some(b'(') => { depth += 1; buf.push(b'('); }
                Some(b'\\') => {
                    match self.advance() {
                        Some(b'n')  => buf.push(b'\n'),
                        Some(b'r')  => buf.push(b'\r'),
                        Some(b't')  => buf.push(b'\t'),
                        Some(b'b')  => buf.push(0x08),
                        Some(b'f')  => buf.push(0x0C),
                        Some(b'\\') => buf.push(b'\\'),
                        Some(b'(')  => buf.push(b'('),
                        Some(b')')  => buf.push(b')'),
                        // Octal escape: up to three octal digits.
                        Some(d @ b'0'..=b'7') => {
                            let mut val = (d - b'0') as u32;
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d2 @ b'0'..=b'7') => {
                                        val = val * 8 + (d2 - b'0') as u32;
                                        self.advance();
                                    }
                                    _ => break,
                                }
                            }
                            buf.push(val as u8);
                        }
                        // Backslash followed by newline: line continuation, ignored.
                        Some(b'\n') | Some(b'\r') => {}
                        Some(other) => buf.push(other),
                        None => return Err(LexError::UnterminatedString),
                    }
                }
                Some(b) => buf.push(b),
            }
        }
        Ok(buf)
    }

    /// Try to parse `word` as integer then float; return the appropriate token.
    fn parse_number(word: &str) -> Option<Token> {
        if let Ok(n) = word.parse::<i64>() {
            return Some(Token::Integer(n));
        }
        // PostScript also accepts radix notation: `16#FF`, `8#77`, etc.
        if let Some((base_str, digits)) = word.split_once('#') {
            if let (Ok(base), true) = (
                base_str.parse::<u32>(),
                base_str.chars().all(|c| c.is_ascii_digit()),
            ) {
                if let Ok(n) = i64::from_str_radix(digits, base) {
                    return Some(Token::Integer(n));
                }
            }
        }
        if let Ok(v) = word.parse::<f64>() {
            return Some(Token::Float(v));
        }
        None
    }

    /// Consume and return the next token, or `None` at end of input.
    pub fn next_token(&mut self) -> Result<Option<Token>, LexError> {
        self.skip_whitespace_and_comments();

        let b = match self.peek() {
            None => return Ok(None),
            Some(b) => b,
        };

        match b {
            b'{' => { self.advance(); Ok(Some(Token::LBrace)) }
            b'}' => { self.advance(); Ok(Some(Token::RBrace)) }
            b'[' => { self.advance(); Ok(Some(Token::LBracket)) }
            b']' => { self.advance(); Ok(Some(Token::RBracket)) }

            b'(' => {
                self.advance();
                Ok(Some(Token::PSString(self.lex_string()?)))
            }

            // Literal name: `/foo` or `//foo` (immediately evaluated — treat same for now).
            b'/' => {
                self.advance();
                // Consume optional second `/` (immediately evaluated name — keep as literal).
                if self.peek() == Some(b'/') { self.advance(); }
                let word = self.take_while(is_name_char);
                Ok(Some(Token::LiteralName(String::from_utf8_lossy(&word).into_owned())))
            }

            _ => {
                // Collect a full "word" token (name or number).
                let word_bytes = self.take_while(is_name_char);
                let word = String::from_utf8_lossy(&word_bytes).into_owned();

                if word == "true"  { return Ok(Some(Token::Boolean(true))); }
                if word == "false" { return Ok(Some(Token::Boolean(false))); }

                if let Some(tok) = Self::parse_number(&word) {
                    return Ok(Some(tok));
                }

                Ok(Some(Token::ExecutableName(word)))
            }
        }
    }

    /// Collect all tokens eagerly. Useful for tests.
    pub fn tokenize(input: &str) -> Result<Vec<Token>, LexError> {
        let mut lexer = Lexer::new(input);
        let mut tokens = Vec::new();
        while let Some(tok) = lexer.next_token()? {
            tokens.push(tok);
        }
        Ok(tokens)
    }
}

/// A byte is a valid name character if it is not whitespace and not a PS delimiter.
fn is_name_char(b: u8) -> bool {
    !b.is_ascii_whitespace() && !matches!(b, b'(' | b')' | b'{' | b'}' | b'[' | b']' | b'/' | b'%' | b'<' | b'>')
}

/// Errors the lexer can produce.
#[derive(Debug, thiserror::Error)]
pub enum LexError {
    #[error("unterminated string literal")]
    UnterminatedString,
    #[error("unexpected character: {0:?}")]
    UnexpectedChar(char),
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers() {
        let toks = Lexer::tokenize("42 -7 0").unwrap();
        assert_eq!(toks, vec![Token::Integer(42), Token::Integer(-7), Token::Integer(0)]);
    }

    #[test]
    fn floats() {
        let toks = Lexer::tokenize("3.14 -0.5 1e3").unwrap();
        assert_eq!(toks, vec![Token::Float(3.14), Token::Float(-0.5), Token::Float(1000.0)]);
    }

    #[test]
    fn booleans() {
        let toks = Lexer::tokenize("true false").unwrap();
        assert_eq!(toks, vec![Token::Boolean(true), Token::Boolean(false)]);
    }

    #[test]
    fn literal_name() {
        let toks = Lexer::tokenize("/foo /bar").unwrap();
        assert_eq!(toks, vec![
            Token::LiteralName("foo".into()),
            Token::LiteralName("bar".into()),
        ]);
    }

    #[test]
    fn executable_name() {
        let toks = Lexer::tokenize("add dup").unwrap();
        assert_eq!(toks, vec![
            Token::ExecutableName("add".into()),
            Token::ExecutableName("dup".into()),
        ]);
    }

    #[test]
    fn ps_string_simple() {
        let toks = Lexer::tokenize("(hello world)").unwrap();
        assert_eq!(toks, vec![Token::PSString(b"hello world".to_vec())]);
    }

    #[test]
    fn ps_string_escape_sequences() {
        let toks = Lexer::tokenize(r"(line1\nline2)").unwrap();
        assert_eq!(toks, vec![Token::PSString(b"line1\nline2".to_vec())]);
    }

    #[test]
    fn ps_string_nested_parens() {
        let toks = Lexer::tokenize("(a(b)c)").unwrap();
        assert_eq!(toks, vec![Token::PSString(b"a(b)c".to_vec())]);
    }

    #[test]
    fn procedure_braces() {
        let toks = Lexer::tokenize("{ add 2 mul }").unwrap();
        assert_eq!(toks, vec![
            Token::LBrace,
            Token::ExecutableName("add".into()),
            Token::Integer(2),
            Token::ExecutableName("mul".into()),
            Token::RBrace,
        ]);
    }

    #[test]
    fn comment_skipped() {
        let toks = Lexer::tokenize("1 % this is a comment\n2").unwrap();
        assert_eq!(toks, vec![Token::Integer(1), Token::Integer(2)]);
    }

    #[test]
    fn radix_notation() {
        let toks = Lexer::tokenize("16#FF 8#10").unwrap();
        assert_eq!(toks, vec![Token::Integer(255), Token::Integer(8)]);
    }
}
