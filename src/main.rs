mod dict_stack;
mod interpreter;
mod lexer;
mod ops;
mod parser;
mod stack;
mod types;
#[cfg(test)]
mod tests;

use interpreter::Interpreter;
use parser::parse;
use std::io::{self, BufRead, Write};
use types::PSError;

fn main() {
    let mut interp = Interpreter::new();
    interp.register_builtins();

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let mut pending = String::new();

    prompt(&mut out, pending.is_empty());
    for raw in stdin.lock().lines() {
        let line = match raw {
            Ok(l) => l,
            Err(e) => { eprintln!("input error: {e}"); break; }
        };

        pending.push_str(&line);
        pending.push('\n');

        // Wait until all `{` are closed before executing, so multi-line
        // procedure definitions work correctly.
        if !braces_balanced(&pending) {
            prompt(&mut out, false);
            continue;
        }

        let src = pending.trim().to_string();
        pending.clear();

        if !src.is_empty() {
            match parse(&src) {
                Err(e) => eprintln!("parse error: {e}"),
                Ok(program) => match interp.exec_body(&program, None) {
                    Ok(()) => {}
                    Err(PSError::Quit) => {
                        writeln!(out, "quit").ok();
                        return;
                    }
                    Err(e) => eprintln!("error: {e}"),
                },
            }
        }

        prompt(&mut out, true);
    }

    // Newline after EOF so the shell prompt appears on its own line.
    writeln!(out).ok();
}

fn prompt(out: &mut impl Write, fresh: bool) {
    if fresh { write!(out, "PS> ").ok(); } else { write!(out, "... ").ok(); }
    out.flush().ok();
}

/// Returns true when every `{` in `s` has a matching `}`.
/// Used to detect incomplete multi-line procedure bodies.
fn braces_balanced(s: &str) -> bool {
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut in_comment = false;
    let mut prev = b'\0';

    for &b in s.as_bytes() {
        if in_comment {
            if b == b'\n' { in_comment = false; }
            prev = b;
            continue;
        }
        if in_string {
            match b {
                b')' if prev != b'\\' => in_string = false,
                _ => {}
            }
            prev = b;
            continue;
        }
        match b {
            b'%' => in_comment = true,
            b'(' => in_string = true,
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        prev = b;
    }
    depth <= 0
}
