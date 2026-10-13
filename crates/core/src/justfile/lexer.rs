use std::fmt::{self, Display, Formatter};
use std::str::Chars;

use super::CompileError;

type Result<T = ()> = std::result::Result<T, CompileError>;

pub const INTERPOLATION_END: &str = "}}";
pub const INTERPOLATION_ESCAPE: &str = "{{{{";
pub const INTERPOLATION_START: &str = "{{";

#[derive(Debug, PartialEq, Clone, Copy, Ord, PartialOrd, Eq)]
pub enum TokenKind {
    AmpersandAmpersand,
    Asterisk,
    At,
    Backtick,
    Bang,
    BangEquals,
    BangTilde,
    BarBar,
    BraceL,
    BraceR,
    BracketL,
    BracketR,
    ByteOrderMark,
    Colon,
    ColonColon,
    ColonEquals,
    Comma,
    Comment,
    Dedent,
    Dollar,
    Eof,
    Eol,
    Equals,
    EqualsEquals,
    EqualsTilde,
    FormatStringContinue,
    FormatStringEnd,
    FormatStringStart,
    Identifier,
    Indent,
    InterpolationEnd,
    InterpolationStart,
    ParenL,
    ParenR,
    Plus,
    PlusPlus,
    QuestionMark,
    Slash,
    StringToken,
    Text,
    Unspecified,
    Whitespace,
}

impl Display for TokenKind {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let name = match self {
            Self::AmpersandAmpersand => "'&&'",
            Self::Asterisk => "'*'",
            Self::At => "'@'",
            Self::Backtick => "backtick",
            Self::Bang => "'!'",
            Self::BangEquals => "'!='",
            Self::BangTilde => "'!~'",
            Self::BarBar => "'||'",
            Self::BraceL => "'{'",
            Self::BraceR => "'}'",
            Self::BracketL => "'['",
            Self::BracketR => "']'",
            Self::ByteOrderMark => "byte order mark",
            Self::Colon => "':'",
            Self::ColonColon => "'::'",
            Self::ColonEquals => "':='",
            Self::Comma => "','",
            Self::Comment => "comment",
            Self::Dedent => "dedent",
            Self::Dollar => "'$'",
            Self::Eof => "end of file",
            Self::Eol => "end of line",
            Self::Equals => "'='",
            Self::EqualsEquals => "'=='",
            Self::EqualsTilde => "'=~'",
            Self::FormatStringContinue => "format string continue",
            Self::FormatStringEnd => "format string end",
            Self::FormatStringStart => "format string",
            Self::Identifier => "identifier",
            Self::Indent => "indent",
            Self::InterpolationEnd => "'}}'",
            Self::InterpolationStart => "'{{'",
            Self::ParenL => "'('",
            Self::ParenR => "')'",
            Self::Plus => "'+'",
            Self::PlusPlus => "'++'",
            Self::QuestionMark => "'?'",
            Self::Slash => "'/'",
            Self::StringToken => "string",
            Self::Text => "command text",
            Self::Unspecified => "unspecified",
            Self::Whitespace => "whitespace",
        };
        write!(f, "{name}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
pub struct Token<'src> {
    pub column: usize,
    pub kind: TokenKind,
    pub length: usize,
    pub line: usize,
    pub offset: usize,
    pub lexeme: &'src str,
}

impl Token<'_> {
    pub fn error(&self, message: impl Into<String>) -> CompileError {
        CompileError::new(*self, message)
    }
}

impl Display for Token<'_> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.lexeme)
    }
}

#[derive(Debug, PartialEq, Clone, Copy, Ord, PartialOrd, Eq)]
pub enum StringDelimiter {
    Backtick,
    QuoteDouble,
    QuoteSingle,
}

#[derive(Debug, PartialEq, Clone, Copy, Ord, PartialOrd, Eq)]
pub struct StringKind {
    pub delimiter: StringDelimiter,
    pub indented: bool,
}

impl StringKind {
    const ALL: [Self; 6] = [
        Self::new(StringDelimiter::Backtick, true),
        Self::new(StringDelimiter::Backtick, false),
        Self::new(StringDelimiter::QuoteDouble, true),
        Self::new(StringDelimiter::QuoteDouble, false),
        Self::new(StringDelimiter::QuoteSingle, true),
        Self::new(StringDelimiter::QuoteSingle, false),
    ];

    const fn new(delimiter: StringDelimiter, indented: bool) -> Self {
        Self {
            delimiter,
            indented,
        }
    }

    pub fn delimiter(self) -> &'static str {
        match (self.delimiter, self.indented) {
            (StringDelimiter::Backtick, false) => "`",
            (StringDelimiter::Backtick, true) => "```",
            (StringDelimiter::QuoteDouble, false) => "\"",
            (StringDelimiter::QuoteDouble, true) => "\"\"\"",
            (StringDelimiter::QuoteSingle, false) => "'",
            (StringDelimiter::QuoteSingle, true) => "'''",
        }
    }

    fn token_kind(self) -> TokenKind {
        match self.delimiter {
            StringDelimiter::QuoteDouble | StringDelimiter::QuoteSingle => TokenKind::StringToken,
            StringDelimiter::Backtick => TokenKind::Backtick,
        }
    }

    fn unterminated_message(self) -> &'static str {
        match self.delimiter {
            StringDelimiter::QuoteDouble | StringDelimiter::QuoteSingle => "unterminated string",
            StringDelimiter::Backtick => "unterminated backtick",
        }
    }

    pub fn processes_escape_sequences(self) -> bool {
        match self.delimiter {
            StringDelimiter::QuoteDouble => true,
            StringDelimiter::Backtick | StringDelimiter::QuoteSingle => false,
        }
    }

    pub fn from_token_start(token_start: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| token_start.starts_with(kind.delimiter()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Delimiter {
    Brace,
    Bracket,
    FormatString(StringKind),
    Paren,
}

impl Delimiter {
    fn open(self) -> &'static str {
        match self {
            Self::Brace => "{",
            Self::Bracket => "[",
            Self::FormatString(_) => "{{",
            Self::Paren => "(",
        }
    }

    fn close(self) -> &'static str {
        match self {
            Self::Brace => "}",
            Self::Bracket => "]",
            Self::FormatString(_) => "}}",
            Self::Paren => ")",
        }
    }
}

#[derive(Clone, Copy)]
struct Position {
    offset: usize,
    column: usize,
    line: usize,
}

enum Indentation<'src> {
    Blank,
    Continue,
    Decrease,
    Inconsistent,
    Increase,
    Mixed { whitespace: &'src str },
}

enum Terminator {
    EndOfFile,
    Interpolation,
    Newline,
    NewlineCarriageReturn,
}

pub fn lex(src: &str) -> Result<Vec<Token<'_>>> {
    let mut chars = src.chars();
    let next = chars.next();
    let start = Position {
        offset: 0,
        column: 0,
        line: 0,
    };
    Lexer {
        chars,
        dedent_scan: (0, false),
        indentation: vec![""],
        interpolation_stack: Vec::new(),
        lines: src.lines().count(),
        next,
        open_delimiters: Vec::new(),
        recipe_body: false,
        recipe_body_pending: false,
        src,
        token_end: start,
        token_start: start,
        tokens: Vec::new(),
    }
    .tokenize()
}

fn is_identifier_start(c: char) -> bool {
    matches!(c, 'a'..='z' | 'A'..='Z' | '_')
}

fn is_identifier_continue(c: char) -> bool {
    is_identifier_start(c) || matches!(c, '0'..='9' | '-')
}

fn show_whitespace(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\t' => '␉',
            ' ' => '␠',
            other => other,
        })
        .collect()
}

struct Lexer<'src> {
    chars: Chars<'src>,
    dedent_scan: (usize, bool),
    indentation: Vec<&'src str>,
    interpolation_stack: Vec<Token<'src>>,
    lines: usize,
    next: Option<char>,
    open_delimiters: Vec<(Delimiter, usize)>,
    recipe_body: bool,
    recipe_body_pending: bool,
    src: &'src str,
    token_end: Position,
    token_start: Position,
    tokens: Vec<Token<'src>>,
}

impl<'src> Lexer<'src> {
    fn advance(&mut self) -> Result {
        match self.next {
            Some(c) => {
                let len_utf8 = c.len_utf8();
                self.token_end.offset += len_utf8;
                self.token_end.column += len_utf8;
                if c == '\n' && self.token_end.line + 1 < self.lines {
                    self.token_end.column = 0;
                    self.token_end.line += 1;
                }
                self.next = self.chars.next();
                Ok(())
            }
            None => Err(self.internal_error("lexer advanced past end of text")),
        }
    }

    fn lexeme(&self) -> &'src str {
        &self.src[self.token_start.offset..self.token_end.offset]
    }

    fn current_token_length(&self) -> usize {
        self.token_end.offset - self.token_start.offset
    }

    fn accepted(&mut self, c: char) -> Result<bool> {
        if self.next_is(c) {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn presume(&mut self, c: char) -> Result {
        if !self.next_is(c) {
            return Err(self.internal_error(format!("lexer presumed character `{c}`")));
        }
        self.advance()
    }

    fn presume_str(&mut self, s: &str) -> Result {
        for c in s.chars() {
            self.presume(c)?;
        }
        Ok(())
    }

    fn next_is(&self, c: char) -> bool {
        self.next == Some(c)
    }

    fn next_is_whitespace(&self) -> bool {
        self.next_is(' ') || self.next_is('\t')
    }

    fn rest(&self) -> &'src str {
        &self.src[self.token_end.offset..]
    }

    fn rest_starts_with(&self, prefix: &str) -> bool {
        self.rest().starts_with(prefix)
    }

    fn at_eol(&self) -> bool {
        self.next_is('\n') || self.rest_starts_with("\r\n")
    }

    fn at_eof(&self) -> bool {
        self.rest().is_empty()
    }

    fn at_eol_or_eof(&self) -> bool {
        self.at_eol() || self.at_eof()
    }

    fn indentation(&self) -> &'src str {
        self.indentation.last().copied().unwrap_or("")
    }

    fn indented(&self) -> bool {
        !self.indentation().is_empty()
    }

    fn token(&mut self, kind: TokenKind) {
        self.tokens.push(Token {
            column: self.token_start.column,
            kind,
            length: self.token_end.offset - self.token_start.offset,
            line: self.token_start.line,
            offset: self.token_start.offset,
            lexeme: self.lexeme(),
        });
        self.token_start = self.token_end;
    }

    fn internal_error(&self, message: impl Into<String>) -> CompileError {
        CompileError {
            line: self.token_end.line,
            column: self.token_end.column,
            message: format!("internal error: {}", message.into()),
        }
    }

    fn error(&self, message: impl Into<String>) -> CompileError {
        CompileError {
            line: self.token_start.line,
            column: self.token_start.column,
            message: message.into(),
        }
    }

    fn tokenize(mut self) -> Result<Vec<Token<'src>>> {
        loop {
            if self.token_start.column == 0 {
                self.lex_line_start()?;
            }

            match self.next {
                Some(first) => {
                    if let Some(&interpolation_start) = self.interpolation_stack.last() {
                        self.lex_interpolation(interpolation_start, first)?;
                    } else if self.recipe_body {
                        self.lex_body()?;
                    } else {
                        self.lex_normal(first)?;
                    }
                }
                None => break,
            }
        }

        if let Some(interpolation_start) = self.interpolation_stack.last() {
            return Err(interpolation_start.error("unterminated interpolation"));
        }

        while self.indented() {
            self.lex_dedent()?;
        }

        self.token(TokenKind::Eof);

        if self.token_start.offset != self.src.len() || self.indentation.len() != 1 {
            return Err(self.internal_error("lexer finished in an inconsistent state"));
        }

        Ok(self.tokens)
    }

    fn lex_line_start(&mut self) -> Result {
        let nonblank_index = self
            .rest()
            .find(|c| !matches!(c, ' ' | '\t'))
            .unwrap_or(self.rest().len());

        let rest = &self.rest()[nonblank_index..];

        let whitespace = &self.rest()[..nonblank_index];

        if self.open_delimiters_or_interpolation() {
            if !whitespace.is_empty() {
                while self.next_is_whitespace() {
                    self.advance()?;
                }
                self.token(TokenKind::Whitespace);
            }
            return Ok(());
        }

        let indentation = whitespace
            .char_indices()
            .nth(self.indentation().chars().count())
            .map_or(whitespace.len(), |(i, _c)| i);

        let body_whitespace = &whitespace[..indentation];

        let spaces = whitespace.chars().any(|c| c == ' ');
        let tabs = whitespace.chars().any(|c| c == '\t');

        let body_spaces = body_whitespace.chars().any(|c| c == ' ');
        let body_tabs = body_whitespace.chars().any(|c| c == '\t');

        let indentation = if rest.starts_with('\n') || rest.starts_with("\r\n") || rest.is_empty() {
            Indentation::Blank
        } else if whitespace == self.indentation() {
            Indentation::Continue
        } else if self.indentation.contains(&whitespace) {
            Indentation::Decrease
        } else if self.recipe_body && whitespace.starts_with(self.indentation()) {
            Indentation::Continue
        } else if self.recipe_body && body_spaces && body_tabs {
            Indentation::Mixed {
                whitespace: body_whitespace,
            }
        } else if !self.recipe_body && spaces && tabs {
            Indentation::Mixed { whitespace }
        } else if whitespace.len() < self.indentation().len()
            || (self.recipe_body
                && body_whitespace.len() >= self.indentation().len()
                && !body_whitespace.starts_with(self.indentation()))
            || (whitespace.len() >= self.indentation().len()
                && !whitespace.starts_with(self.indentation()))
        {
            Indentation::Inconsistent
        } else {
            Indentation::Increase
        };

        match indentation {
            Indentation::Blank => {
                if self.starts_dedent() {
                    while self.indented() {
                        self.lex_dedent()?;
                    }
                }
                if !whitespace.is_empty() {
                    while self.next_is_whitespace() {
                        self.advance()?;
                    }
                    self.token(TokenKind::Whitespace);
                }
                Ok(())
            }
            Indentation::Continue => {
                if !self.indentation().is_empty() {
                    for _ in self.indentation().chars() {
                        self.advance()?;
                    }
                    self.token(TokenKind::Whitespace);
                }
                Ok(())
            }
            Indentation::Decrease => {
                while self.indentation() != whitespace {
                    self.lex_dedent()?;
                }
                if !whitespace.is_empty() {
                    while self.next_is_whitespace() {
                        self.advance()?;
                    }
                    self.token(TokenKind::Whitespace);
                }
                Ok(())
            }
            Indentation::Mixed { whitespace } => {
                for _ in whitespace.chars() {
                    self.advance()?;
                }
                Err(self.error(format!(
                    "found a mix of tabs and spaces in leading whitespace: `{}`",
                    show_whitespace(whitespace)
                )))
            }
            Indentation::Inconsistent => {
                for _ in whitespace.chars() {
                    self.advance()?;
                }
                Err(self.error(format!(
                    "recipe line has inconsistent leading whitespace, started with `{}` but found line with `{}`",
                    show_whitespace(self.indentation()),
                    show_whitespace(whitespace)
                )))
            }
            Indentation::Increase => {
                while self.next_is_whitespace() {
                    self.advance()?;
                }
                let indentation = self.lexeme();
                self.indentation.push(indentation);
                self.token(TokenKind::Indent);
                if self.recipe_body_pending {
                    self.recipe_body = true;
                }
                Ok(())
            }
        }
    }

    fn starts_dedent(&mut self) -> bool {
        let offset = self.token_end.offset;
        if offset >= self.dedent_scan.0 {
            let rest = self.rest();
            self.dedent_scan = match rest.find(|c| !matches!(c, ' ' | '\t' | '\r' | '\n')) {
                Some(index) => (
                    offset + index,
                    index > 0 && rest.as_bytes()[index - 1] == b'\n',
                ),
                None => (self.src.len(), false),
            };
        }
        self.dedent_scan.1
    }

    fn lex_normal(&mut self, start: char) -> Result {
        match start {
            ' ' | '\t' => self.lex_whitespace(),
            '!' => self.lex_choices(
                '!',
                &[('=', TokenKind::BangEquals), ('~', TokenKind::BangTilde)],
                TokenKind::Bang,
            ),
            '#' => self.lex_comment(),
            '$' => self.lex_single(TokenKind::Dollar),
            '&' => self.lex_digraph('&', '&', TokenKind::AmpersandAmpersand),
            '(' => self.lex_delimiter(TokenKind::ParenL),
            ')' => self.lex_delimiter(TokenKind::ParenR),
            '*' => self.lex_single(TokenKind::Asterisk),
            '+' => self.lex_choices('+', &[('+', TokenKind::PlusPlus)], TokenKind::Plus),
            ',' => self.lex_single(TokenKind::Comma),
            '/' => self.lex_single(TokenKind::Slash),
            ':' => self.lex_colon(),
            '=' => self.lex_choices(
                '=',
                &[
                    ('=', TokenKind::EqualsEquals),
                    ('~', TokenKind::EqualsTilde),
                ],
                TokenKind::Equals,
            ),
            '?' => self.lex_single(TokenKind::QuestionMark),
            '@' => self.lex_single(TokenKind::At),
            '[' => self.lex_delimiter(TokenKind::BracketL),
            '\\' => self.lex_escape(),
            '\n' | '\r' => self.lex_eol(),
            '\u{feff}' => self.lex_single(TokenKind::ByteOrderMark),
            ']' => self.lex_delimiter(TokenKind::BracketR),
            '`' | '"' | '\'' => self.lex_string(None),
            '{' => self.lex_delimiter(TokenKind::BraceL),
            '|' => self.lex_digraph('|', '|', TokenKind::BarBar),
            '}' => {
                let format_string_kind = match self.open_delimiters.last() {
                    Some((Delimiter::FormatString(kind), _line))
                        if self.rest_starts_with(INTERPOLATION_END) =>
                    {
                        Some(*kind)
                    }
                    _ => None,
                };
                match format_string_kind {
                    Some(kind) => self.lex_string(Some(kind)),
                    None => self.lex_delimiter(TokenKind::BraceR),
                }
            }
            _ if is_identifier_start(start) => self.lex_identifier(),
            _ => {
                self.advance()?;
                Err(self.error(format!(
                    "unknown start of token '{}'",
                    start.escape_default()
                )))
            }
        }
    }

    fn lex_interpolation(&mut self, interpolation_start: Token<'src>, start: char) -> Result {
        if self.rest_starts_with(INTERPOLATION_END) && self.open_delimiters.is_empty() {
            self.interpolation_stack.pop();
            self.lex_double(TokenKind::InterpolationEnd)
        } else if self.at_eof() && self.open_delimiters.is_empty() {
            Err(interpolation_start.error("unterminated interpolation"))
        } else {
            self.lex_normal(start)
        }
    }

    fn lex_body(&mut self) -> Result {
        let terminator = loop {
            if self.rest_starts_with(INTERPOLATION_ESCAPE) {
                self.presume_str(INTERPOLATION_ESCAPE)?;
                continue;
            }
            if self.rest_starts_with("\n") {
                break Terminator::Newline;
            }
            if self.rest_starts_with("\r\n") {
                break Terminator::NewlineCarriageReturn;
            }
            if self.rest_starts_with(INTERPOLATION_START) {
                break Terminator::Interpolation;
            }
            if self.at_eof() {
                break Terminator::EndOfFile;
            }
            self.advance()?;
        };

        if self.current_token_length() > 0 {
            self.token(TokenKind::Text);
        }

        match terminator {
            Terminator::Newline => self.lex_single(TokenKind::Eol),
            Terminator::NewlineCarriageReturn => self.lex_double(TokenKind::Eol),
            Terminator::Interpolation => {
                self.lex_double(TokenKind::InterpolationStart)?;
                if let Some(&token) = self.tokens.last() {
                    self.interpolation_stack.push(token);
                }
                Ok(())
            }
            Terminator::EndOfFile => Ok(()),
        }
    }

    fn lex_dedent(&mut self) -> Result {
        if self.current_token_length() != 0 {
            return Err(self.internal_error("dedent with non-empty token"));
        }
        self.token(TokenKind::Dedent);
        self.indentation.pop();
        self.recipe_body_pending = false;
        self.recipe_body = false;
        Ok(())
    }

    fn lex_single(&mut self, kind: TokenKind) -> Result {
        self.advance()?;
        self.token(kind);
        Ok(())
    }

    fn lex_double(&mut self, kind: TokenKind) -> Result {
        self.advance()?;
        self.advance()?;
        self.token(kind);
        Ok(())
    }

    fn lex_choices(
        &mut self,
        first: char,
        choices: &[(char, TokenKind)],
        otherwise: TokenKind,
    ) -> Result {
        self.presume(first)?;
        for &(second, then) in choices {
            if self.accepted(second)? {
                self.token(then);
                return Ok(());
            }
        }
        self.token(otherwise);
        Ok(())
    }

    fn lex_delimiter(&mut self, kind: TokenKind) -> Result {
        match kind {
            TokenKind::BraceL => self.open_delimiter(Delimiter::Brace),
            TokenKind::BraceR => self.close_delimiter(Delimiter::Brace)?,
            TokenKind::BracketL => self.open_delimiter(Delimiter::Bracket),
            TokenKind::BracketR => self.close_delimiter(Delimiter::Bracket)?,
            TokenKind::ParenL => self.open_delimiter(Delimiter::Paren),
            TokenKind::ParenR => self.close_delimiter(Delimiter::Paren)?,
            _ => {
                return Err(
                    self.internal_error(format!("lex_delimiter called with non-delimiter {kind}"))
                );
            }
        }
        self.lex_single(kind)
    }

    fn open_delimiter(&mut self, delimiter: Delimiter) {
        self.open_delimiters
            .push((delimiter, self.token_start.line));
    }

    fn close_delimiter(&mut self, close: Delimiter) -> Result {
        match self.open_delimiters.pop() {
            Some((open, _)) if open == close => Ok(()),
            Some((open, open_line)) => Err(self.error(format!(
                "mismatched closing delimiter `{}`, did you mean to close the `{}` on line {}?",
                close.close(),
                open.open(),
                open_line + 1
            ))),
            None => Err(self.error(format!("unexpected closing delimiter `{}`", close.close()))),
        }
    }

    fn open_delimiters_or_interpolation(&self) -> bool {
        !self.open_delimiters.is_empty() || !self.interpolation_stack.is_empty()
    }

    fn lex_digraph(&mut self, left: char, right: char, token: TokenKind) -> Result {
        self.presume(left)?;
        if self.accepted(right)? {
            self.token(token);
            return Ok(());
        }
        self.token(TokenKind::Unspecified);
        if self.at_eof() {
            return Err(self.error(format!(
                "expected character `{right}` but found end-of-file"
            )));
        }
        self.advance()?;
        Err(self.error(format!("expected character `{right}`")))
    }

    fn lex_colon(&mut self) -> Result {
        self.presume(':')?;
        if self.accepted('=')? {
            self.token(TokenKind::ColonEquals);
        } else if self.accepted(':')? {
            self.token(TokenKind::ColonColon);
        } else {
            self.token(TokenKind::Colon);
            self.recipe_body_pending = true;
        }
        Ok(())
    }

    fn lex_escape(&mut self) -> Result {
        self.presume('\\')?;
        if self.accepted('\n')? {
            while self.next_is_whitespace() {
                self.advance()?;
            }
            self.token(TokenKind::Whitespace);
            Ok(())
        } else if self.accepted('\r')? {
            if !self.accepted('\n')? {
                return Err(self.error("unpaired carriage return"));
            }
            while self.next_is_whitespace() {
                self.advance()?;
            }
            self.token(TokenKind::Whitespace);
            Ok(())
        } else if let Some(character) = self.next {
            Err(self.error(format!(
                "`\\{}` is not a valid escape sequence",
                character.escape_default()
            )))
        } else {
            Err(self.error("expected escape sequence but found end-of-file"))
        }
    }

    fn lex_eol(&mut self) -> Result {
        if self.accepted('\r')? {
            if !self.accepted('\n')? {
                return Err(self.error("unpaired carriage return"));
            }
        } else {
            self.presume('\n')?;
        }
        if self.open_delimiters_or_interpolation() {
            self.token(TokenKind::Whitespace);
        } else {
            self.token(TokenKind::Eol);
        }
        Ok(())
    }

    fn lex_identifier(&mut self) -> Result {
        self.advance()?;
        while let Some(c) = self.next {
            if !is_identifier_continue(c) {
                break;
            }
            self.advance()?;
        }
        self.token(TokenKind::Identifier);
        Ok(())
    }

    fn lex_comment(&mut self) -> Result {
        self.presume('#')?;
        while !self.at_eol_or_eof() {
            self.advance()?;
        }
        self.token(TokenKind::Comment);
        Ok(())
    }

    fn lex_whitespace(&mut self) -> Result {
        while self.next_is_whitespace() {
            self.advance()?;
        }
        self.token(TokenKind::Whitespace);
        Ok(())
    }

    fn lex_string(&mut self, format_string_kind: Option<StringKind>) -> Result {
        let kind = match format_string_kind {
            Some(kind) => {
                self.presume_str(INTERPOLATION_END)?;
                kind
            }
            None => {
                let Some(kind) = StringKind::from_token_start(self.rest()) else {
                    self.advance()?;
                    return Err(self.internal_error("invalid string start"));
                };
                self.presume_str(kind.delimiter())?;
                kind
            }
        };

        let format = format_string_kind.is_some()
            || kind.delimiter != StringDelimiter::Backtick
                && self.tokens.last().is_some_and(|token| {
                    token.kind == TokenKind::Identifier && token.lexeme == "f"
                });

        let mut escape = false;

        loop {
            if self.next.is_none() {
                return Err(self.error(kind.unterminated_message()));
            } else if !escape && kind.processes_escape_sequences() && self.next_is('\\') {
                escape = true;
            } else if escape && kind.processes_escape_sequences() && self.next_is('u') {
                escape = false;
            } else if format && self.rest_starts_with(INTERPOLATION_ESCAPE) {
                escape = false;
                self.advance()?;
                self.advance()?;
                self.advance()?;
            } else if !escape
                && (self.rest_starts_with(kind.delimiter())
                    || format && self.rest_starts_with(INTERPOLATION_START))
            {
                break;
            } else {
                escape = false;
            }
            self.advance()?;
        }

        if format && self.rest_starts_with(INTERPOLATION_START) {
            self.presume_str(INTERPOLATION_START)?;
            match format_string_kind {
                Some(_) => self.token(TokenKind::FormatStringContinue),
                None => {
                    self.token(TokenKind::FormatStringStart);
                    self.open_delimiter(Delimiter::FormatString(kind));
                }
            }
        } else {
            self.presume_str(kind.delimiter())?;
            match format_string_kind {
                Some(format_string_kind) => {
                    self.close_delimiter(Delimiter::FormatString(format_string_kind))?;
                    self.token(TokenKind::FormatStringEnd);
                }
                None => self.token(kind.token_kind()),
            }
        }

        Ok(())
    }
}
