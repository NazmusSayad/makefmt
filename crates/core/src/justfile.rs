mod ast;
mod lexer;
mod parser;

use std::fmt::{self, Display, Formatter};

use lexer::Token;

pub fn format_justfile(source: &str) -> Result<String, String> {
    let tokens = lexer::lex(source).map_err(|error| error.to_string())?;
    let items = parser::parse(&tokens).map_err(|error| error.to_string())?;
    ast::format(&items).map_err(|error| error.to_string())
}

#[derive(Debug)]
struct CompileError {
    line: usize,
    column: usize,
    message: String,
}

impl CompileError {
    fn new(token: Token, message: impl Into<String>) -> Self {
        Self {
            line: token.line,
            column: token.column,
            message: message.into(),
        }
    }
}

impl Display for CompileError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(
            f,
            "line {}, column {}: {}",
            self.line + 1,
            self.column + 1,
            self.message
        )
    }
}
