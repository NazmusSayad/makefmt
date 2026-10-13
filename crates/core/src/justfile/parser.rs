use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::CompileError;
use super::ast::{
    ArgAttribute, Attribute, AttributeKind, AttributeSet, ConditionalOperator, Dependency,
    DependencyArgument, Expression, FormatStringPart, Fragment, Interpreter, Item, Parameter,
    ParameterKind, Recipe, Setting, Signal, StringLiteral, contains,
};
use super::lexer::{
    INTERPOLATION_END, INTERPOLATION_ESCAPE, INTERPOLATION_START, StringKind, Token, TokenKind,
};

type Result<T = ()> = std::result::Result<T, CompileError>;

const RECURSION_LIMIT: usize = if cfg!(windows) { 48 } else { 256 };
const NESTING_LIMIT: usize = 1024;
const CURRENT_VERSION: (u64, u64, u64) = (1, 58, 0);

const BOOLEAN_SETTINGS: [&str; 19] = [
    "allow-duplicate-recipes",
    "allow-duplicate-variables",
    "default-list",
    "default-script",
    "dotenv-load",
    "dotenv-override",
    "dotenv-required",
    "export",
    "fallback",
    "guards",
    "ignore-comments",
    "lazy",
    "lists",
    "no-cd",
    "no-exit-message",
    "positional-arguments",
    "quiet",
    "unstable",
    "windows-powershell",
];

const EXPRESSION_SETTINGS: [&str; 5] = [
    "dotenv-command",
    "dotenv-filename",
    "dotenv-path",
    "tempdir",
    "working-directory",
];

const INTERPRETER_SETTINGS: [&str; 3] = ["script-interpreter", "shell", "windows-shell"];

#[derive(Clone, Copy)]
enum StringState {
    FormatContinue(StringKind),
    FormatStart,
    Normal,
}

struct ArgInfo<'src> {
    flag: bool,
    multiple: bool,
    count_key: Option<Token<'src>>,
    name: Token<'src>,
}

type KeywordArguments<'src> = BTreeMap<&'src str, (Token<'src>, Option<Expression<'src>>)>;

pub fn parse<'src>(tokens: &[Token<'src>]) -> Result<Vec<Item<'src>>> {
    Parser {
        expected_tokens: BTreeSet::new(),
        items: Vec::new(),
        nesting_depth: 0,
        next_token: 0,
        recursion_depth: 0,
        tokens,
    }
    .parse_ast()
}

fn unindent(text: &str) -> String {
    let lines = text.split_inclusive('\n').collect::<Vec<&str>>();

    let common_indentation = lines
        .iter()
        .filter(|line| !blank(line))
        .map(|line| indentation(line))
        .reduce(common)
        .unwrap_or("");

    let mut result = String::new();

    for (i, line) in lines.iter().enumerate() {
        let first = i == 0;
        let last = i == lines.len() - 1;
        match (blank(line), first, last) {
            (true, false, false) => {
                if line.ends_with("\r\n") {
                    result.push_str("\r\n");
                } else {
                    result.push('\n');
                }
            }
            (true, _, _) => {}
            (false, _, _) => result.push_str(&line[common_indentation.len()..]),
        }
    }

    result
}

fn indentation(line: &str) -> &str {
    let end = line
        .find(|c| !matches!(c, ' ' | '\t'))
        .unwrap_or(line.len());
    &line[..end]
}

fn blank(line: &str) -> bool {
    line.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
}

fn common<'s>(a: &'s str, b: &'s str) -> &'s str {
    let end = a
        .char_indices()
        .zip(b.chars())
        .take_while(|((_, ac), bc)| ac == bc)
        .map(|((i, c), _)| i + c.len_utf8())
        .last()
        .unwrap_or(0);
    &a[..end]
}

fn is_number(text: &str) -> bool {
    match text.as_bytes() {
        [b'0'] => true,
        [b'1'..=b'9', rest @ ..] => rest.iter().all(u8::is_ascii_digit),
        _ => false,
    }
}

fn parse_version_component(text: &str) -> Option<u64> {
    if is_number(text) && text.len() <= 9 {
        text.parse().ok()
    } else {
        None
    }
}

fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split('.');
    let major = parse_version_component(parts.next()?)?;
    let minor = parse_version_component(parts.next()?)?;
    let patch = parse_version_component(parts.next()?)?;
    match parts.next() {
        Some(_) => None,
        None => Some((major, minor, patch)),
    }
}

fn parse_indentation(text: &str) -> std::result::Result<String, &'static str> {
    let mut chars = text.chars();
    let Some(character) = chars.next() else {
        return Err("indentation must not be empty");
    };
    if !matches!(character, ' ' | '\t') {
        return Err("indentation must be spaces or tabs");
    }
    if !chars.all(|c| c == character) {
        return Err("indentation may not be mixed");
    }
    Ok(text.to_string())
}

fn ensure_valid_attributes(
    attributes: &AttributeSet,
    item_kind: &str,
    valid: &[AttributeKind],
    item_token: Token,
) -> Result {
    for attribute in attributes {
        let kind = attribute.kind();
        if !kind.is_enabler() && !valid.contains(&kind) {
            return Err(item_token.error(format!(
                "{item_kind} `{}` has invalid attribute `{}`",
                item_token.lexeme,
                kind.name()
            )));
        }
    }
    Ok(())
}

fn take_attributes<'src>(
    attributes: &mut Option<(Token<'src>, AttributeSet<'src>)>,
) -> AttributeSet<'src> {
    match attributes.take() {
        Some((_token, attributes)) => attributes,
        None => AttributeSet::new(),
    }
}

fn require_string_literal<'src>(
    attribute: Token<'src>,
    key: Token<'src>,
    expression: Expression<'src>,
) -> Result<StringLiteral<'src>> {
    match expression {
        Expression::StringLiteral { string_literal } => Ok(string_literal),
        _ => Err(key.error(format!(
            "attribute `{}` arguments must be string literals",
            attribute.lexeme
        ))),
    }
}

fn remove_required<'src>(
    keyword_arguments: &mut KeywordArguments<'src>,
    key: &str,
) -> Result<Option<(Token<'src>, Expression<'src>)>> {
    match keyword_arguments.remove(key) {
        None => Ok(None),
        Some((key, Some(expression))) => Ok(Some((key, expression))),
        Some((key, None)) => {
            Err(key.error(format!("attribute key `{}` requires value", key.lexeme)))
        }
    }
}

fn check_option_name(parameter: &StringLiteral, literal: &StringLiteral) -> Result {
    if literal.cooked.contains('=') {
        return Err(literal.token.error(format!(
            "option name for parameter `{}` contains equal sign",
            parameter.cooked
        )));
    }
    if literal.cooked.starts_with('-') {
        return Err(literal.token.error(format!(
            "option name for parameter `{}` starts with `-`",
            parameter.cooked
        )));
    }
    if literal.cooked.is_empty() {
        return Err(literal.token.error(format!(
            "option name for parameter `{}` is empty",
            parameter.cooked
        )));
    }
    Ok(())
}

fn parse_count<'src>(
    attribute: Token<'src>,
    key: Token<'src>,
    expression: Expression<'src>,
) -> Result<u64> {
    let literal = require_string_literal(attribute, key, expression)?;
    if !is_number(&literal.cooked) {
        return Err(literal.token.error(format!(
            "invalid `{}` value `{}`",
            key.lexeme, literal.cooked
        )));
    }
    literal.cooked.parse::<u64>().map_err(|error| {
        literal.token.error(format!(
            "invalid `{}` value `{}`: {error}",
            key.lexeme, literal.cooked
        ))
    })
}

fn new_arg<'src>(
    name: Token<'src>,
    arguments: Vec<StringLiteral<'src>>,
    keyword_arguments: &mut KeywordArguments<'src>,
) -> Result<Attribute<'src>> {
    let Some(arg) = arguments.into_iter().next() else {
        return Err(name.error("internal error: `arg` attribute without argument"));
    };

    let (long, long_key) = match keyword_arguments.remove("long") {
        None => (None, None),
        Some((key, None)) => (Some(arg.clone()), Some(key)),
        Some((key, Some(expression))) => {
            let literal = require_string_literal(name, key, expression)?;
            check_option_name(&arg, &literal)?;
            (Some(literal), None)
        }
    };

    let (short, short_key) = match keyword_arguments.remove("short") {
        None => (None, None),
        Some((key, None)) => (Some(arg.clone()), Some(key)),
        Some((key, Some(expression))) => {
            let literal = require_string_literal(name, key, expression)?;
            check_option_name(&arg, &literal)?;
            if literal.cooked.chars().count() != 1 {
                return Err(literal.token.error(format!(
                    "short option for parameter `{}` has multiple characters",
                    arg.cooked
                )));
            }
            (Some(literal), None)
        }
    };

    let pattern_property = remove_required(keyword_arguments, "pattern")?;

    let value = match remove_required(keyword_arguments, "value")? {
        None => None,
        Some((key, expression)) => {
            if long.is_none() && short.is_none() {
                return Err(key.error(format!(
                    "argument attribute `{}` only valid with `long` or `short`",
                    key.lexeme
                )));
            }
            Some(expression)
        }
    };

    let flag = match keyword_arguments.remove("flag") {
        None => None,
        Some((key, expression)) => {
            if expression.is_some() {
                return Err(key.error(format!(
                    "`flag` attribute for argument `{}` takes no value",
                    arg.cooked
                )));
            }
            if long.is_none() && short.is_none() {
                return Err(key.error(format!(
                    "argument attribute `{}` only valid with `long` or `short`",
                    key.lexeme
                )));
            }
            if value.is_some() {
                return Err(key.error(format!(
                    "argument `{}` may not have both `flag` and `value` attributes",
                    arg.cooked
                )));
            }
            if pattern_property.is_some() {
                return Err(key.error(format!(
                    "argument `{}` may not have both `flag` and `pattern` attributes",
                    arg.cooked
                )));
            }
            Some(key)
        }
    };

    let multiple = match keyword_arguments.remove("multiple") {
        None => None,
        Some((key, expression)) => {
            if expression.is_some() {
                return Err(key.error(format!("attribute key `{}` takes no value", key.lexeme)));
            }
            if long.is_none() && short.is_none() {
                return Err(key.error(format!(
                    "argument attribute `{}` only valid with `long` or `short`",
                    key.lexeme
                )));
            }
            Some(key)
        }
    };

    let (max, max_key) = match remove_required(keyword_arguments, "max")? {
        None => (None, None),
        Some((key, expression)) => (Some(parse_count(name, key, expression)?), Some(key)),
    };

    let (min, min_key) = match remove_required(keyword_arguments, "min")? {
        None => (None, None),
        Some((key, expression)) => (Some(parse_count(name, key, expression)?), Some(key)),
    };

    if let (Some(min), Some(max), Some(min_key)) = (min, max, min_key)
        && min > max
    {
        return Err(min_key.error(format!(
            "argument attribute `min` `{min}` exceeds `max` `{max}`"
        )));
    }

    let help_property = remove_required(keyword_arguments, "help")?;

    Ok(Attribute::Arg(Box::new(ArgAttribute {
        flag,
        help_property,
        long,
        long_key,
        max,
        max_key,
        min,
        min_key,
        multiple,
        name: arg,
        pattern_property,
        short,
        short_key,
        value,
    })))
}

fn new_attribute<'src>(
    name: Token<'src>,
    kind: AttributeKind,
    arguments: Vec<(Token<'src>, Expression<'src>)>,
    mut keyword_arguments: KeywordArguments<'src>,
) -> Result<Attribute<'src>> {
    let found = arguments.len();
    let (min, max) = kind.argument_range();
    if found < min || found > max {
        return Err(name.error(format!(
            "attribute `{}` got {found} argument(s) but takes {}",
            name.lexeme,
            if min == max {
                min.to_string()
            } else if found < min {
                format!("at least {min}")
            } else {
                format!("at most {max}")
            }
        )));
    }

    if kind.accepts_expressions() {
        if let Some((_name, (key, _value))) = keyword_arguments.pop_first() {
            return Err(key.error(format!(
                "unknown key `{}` for attribute `{}`",
                key.lexeme, name.lexeme
            )));
        }

        let mut expressions = arguments.into_iter().map(|(_token, expression)| expression);

        return match kind {
            AttributeKind::Confirm => Ok(Attribute::Confirm(expressions.next())),
            AttributeKind::Doc => Ok(Attribute::Doc(expressions.next())),
            AttributeKind::Timestamp => Ok(Attribute::Timestamp(expressions.next())),
            AttributeKind::Env => match (expressions.next(), expressions.next()) {
                (Some(key), Some(value)) => Ok(Attribute::Env(key, value)),
                _ => Err(name.error("internal error: `env` attribute without two arguments")),
            },
            AttributeKind::WorkingDirectory => {
                match expressions.next() {
                    Some(directory) => Ok(Attribute::WorkingDirectory(directory)),
                    None => Err(name
                        .error("internal error: `working-directory` attribute without argument")),
                }
            }
            _ => Err(name.error(format!(
                "internal error: attribute `{}` does not accept expressions",
                name.lexeme
            ))),
        };
    }

    let mut literals = Vec::new();
    for (token, argument) in arguments {
        match argument {
            Expression::StringLiteral { string_literal } => literals.push(string_literal),
            _ => {
                return Err(token.error(format!(
                    "attribute `{}` arguments must be string literals",
                    name.lexeme
                )));
            }
        }
    }

    let attribute = match kind {
        AttributeKind::Arg => new_arg(name, literals, &mut keyword_arguments)?,
        AttributeKind::Android => Attribute::Android,
        AttributeKind::Cache => Attribute::Cache {
            extra: remove_required(&mut keyword_arguments, "extra")?
                .map(|(_key, expression)| expression),
            inputs: remove_required(&mut keyword_arguments, "inputs")?
                .map(|(_key, expression)| expression),
            outputs: remove_required(&mut keyword_arguments, "outputs")?
                .map(|(_key, expression)| expression),
        },
        AttributeKind::Continue => {
            let mut signals = BTreeSet::new();
            for literal in literals {
                let signal = match literal.cooked.as_str() {
                    "SIGHUP" => Signal::Hangup,
                    "SIGINT" => Signal::Interrupt,
                    "SIGQUIT" => Signal::Quit,
                    _ => {
                        return Err(literal.token.error(format!(
                            "invalid signal `{}`: expected `SIGHUP`, `SIGINT`, or `SIGQUIT`",
                            literal.cooked
                        )));
                    }
                };
                signals.insert(signal);
            }
            Attribute::Continue(signals)
        }
        AttributeKind::Default => Attribute::Default,
        AttributeKind::Dragonfly => Attribute::Dragonfly,
        AttributeKind::ExitMessage => Attribute::ExitMessage,
        AttributeKind::Extension => match literals.into_iter().next() {
            Some(extension) => Attribute::Extension(extension),
            None => {
                return Err(name.error("internal error: `extension` attribute without argument"));
            }
        },
        AttributeKind::Freebsd => Attribute::Freebsd,
        AttributeKind::Group => match literals.into_iter().next() {
            Some(group) => Attribute::Group(group),
            None => return Err(name.error("internal error: `group` attribute without argument")),
        },
        AttributeKind::Linux => Attribute::Linux,
        AttributeKind::Macos => Attribute::Macos,
        AttributeKind::Metadata => Attribute::Metadata(literals),
        AttributeKind::Netbsd => Attribute::Netbsd,
        AttributeKind::NoCd => Attribute::NoCd,
        AttributeKind::NoExitMessage => Attribute::NoExitMessage,
        AttributeKind::NoQuiet => Attribute::NoQuiet,
        AttributeKind::Openbsd => Attribute::Openbsd,
        AttributeKind::Parallel => Attribute::Parallel,
        AttributeKind::PositionalArguments => Attribute::PositionalArguments,
        AttributeKind::Private => Attribute::Private,
        AttributeKind::Script => {
            let mut literals = literals.into_iter();
            Attribute::Script(literals.next().map(|command| Interpreter {
                command,
                arguments: literals.collect(),
            }))
        }
        AttributeKind::Shell => Attribute::Shell,
        AttributeKind::Unix => Attribute::Unix,
        AttributeKind::Windows => Attribute::Windows,
        AttributeKind::Confirm
        | AttributeKind::Doc
        | AttributeKind::Env
        | AttributeKind::Timestamp
        | AttributeKind::WorkingDirectory => {
            return Err(name.error(format!(
                "internal error: attribute `{}` accepts expressions",
                name.lexeme
            )));
        }
    };

    if let Some((_name, (key, _value))) = keyword_arguments.pop_first() {
        return Err(key.error(format!(
            "unknown key `{}` for attribute `{}`",
            key.lexeme, name.lexeme
        )));
    }

    Ok(attribute)
}

fn cook_string(token: Token, text: &str) -> Result<String> {
    enum State {
        Backslash,
        BackslashCarriageReturn,
        Initial,
        Unicode,
        UnicodeValue { hex: String },
    }

    let invalid_escape = |character: char| {
        token.error(format!(
            "`\\{}` is not a valid escape sequence",
            character.escape_default()
        ))
    };

    let mut cooked = String::new();
    let mut state = State::Initial;

    for c in text.chars() {
        match state {
            State::Initial => {
                if c == '\\' {
                    state = State::Backslash;
                } else {
                    cooked.push(c);
                }
            }
            State::Backslash if c == 'u' => state = State::Unicode,
            State::Backslash => {
                state = State::Initial;
                match c {
                    'n' => cooked.push('\n'),
                    'r' => cooked.push('\r'),
                    't' => cooked.push('\t'),
                    '\\' => cooked.push('\\'),
                    '\n' => {}
                    '\r' => state = State::BackslashCarriageReturn,
                    '"' => cooked.push('"'),
                    character => return Err(invalid_escape(character)),
                }
            }
            State::BackslashCarriageReturn => match c {
                '\n' => state = State::Initial,
                _ => return Err(invalid_escape('\r')),
            },
            State::Unicode => match c {
                '{' => state = State::UnicodeValue { hex: String::new() },
                character => {
                    return Err(token.error(format!(
                        "expected unicode escape sequence delimiter `{{` but found `{}`",
                        character.escape_default()
                    )));
                }
            },
            State::UnicodeValue { ref mut hex } => match c {
                '}' => {
                    if hex.is_empty() {
                        return Err(token.error("unicode escape sequences must not be empty"));
                    }
                    let character = u32::from_str_radix(hex, 16)
                        .ok()
                        .and_then(char::from_u32)
                        .ok_or_else(|| {
                            token.error(format!(
                                "unicode escape sequence value `{hex}` greater than maximum valid code point `10FFFF`"
                            ))
                        })?;
                    cooked.push(character);
                    state = State::Initial;
                }
                '0'..='9' | 'A'..='F' | 'a'..='f' => {
                    hex.push(c);
                    if hex.len() > 6 {
                        return Err(token.error(format!(
                            "unicode escape sequence starting with `\\u{{{hex}` longer than six hex digits"
                        )));
                    }
                }
                _ => {
                    return Err(token.error(format!(
                        "unicode escape sequence contained character `{}` which is not a valid hex digit",
                        c.escape_default()
                    )));
                }
            },
        }
    }

    match state {
        State::Initial => Ok(cooked),
        State::BackslashCarriageReturn => Err(invalid_escape('\r')),
        State::Backslash | State::Unicode | State::UnicodeValue { .. } => {
            Err(token.error("unterminated unicode escape sequence"))
        }
    }
}

struct Parser<'run, 'src> {
    expected_tokens: BTreeSet<TokenKind>,
    items: Vec<Item<'src>>,
    nesting_depth: usize,
    next_token: usize,
    recursion_depth: usize,
    tokens: &'run [Token<'src>],
}

impl<'run, 'src> Parser<'run, 'src> {
    #[inline(never)]
    fn internal_error(&self, message: &str) -> CompileError {
        let message = format!("internal error: {message}");
        match self.next() {
            Ok(token) => token.error(message),
            Err(error) => error,
        }
    }

    #[inline(never)]
    fn unexpected_token(&self) -> CompileError {
        let found = match self.next() {
            Ok(token) => token,
            Err(error) => return error,
        };
        let expected = self
            .expected_tokens
            .iter()
            .filter(|kind| **kind != TokenKind::ByteOrderMark)
            .map(ToString::to_string)
            .collect::<Vec<String>>();
        let expected = match expected.as_slice() {
            [] => return found.error(format!("unexpected {}", found.kind)),
            [only] => only.clone(),
            [first, second] => format!("{first} or {second}"),
            [rest @ .., last] => format!("{}, or {last}", rest.join(", ")),
        };
        found.error(format!("expected {expected}, but found {}", found.kind))
    }

    fn rest(&self) -> impl Iterator<Item = Token<'src>> + 'run {
        self.tokens[self.next_token.min(self.tokens.len())..]
            .iter()
            .copied()
            .filter(|token| token.kind != TokenKind::Whitespace)
    }

    fn next(&self) -> Result<Token<'src>> {
        match self.rest().next() {
            Some(token) => Ok(token),
            None => Err(CompileError {
                line: 0,
                column: 0,
                message: "internal error: parser reached end of token stream".to_string(),
            }),
        }
    }

    fn next_is(&mut self, kind: TokenKind) -> bool {
        self.next_are(&[kind])
    }

    fn next_is_keyword(&self, keyword: &str) -> bool {
        self.rest()
            .next()
            .is_some_and(|token| token.kind == TokenKind::Identifier && token.lexeme == keyword)
    }

    fn next_are(&mut self, kinds: &[TokenKind]) -> bool {
        if let Some(&kind) = kinds.first() {
            self.expected_tokens.insert(kind);
        }
        let mut rest = self.rest();
        kinds
            .iter()
            .all(|kind| rest.next().is_some_and(|token| token.kind == *kind))
    }

    fn line_is(&mut self, kinds: &[TokenKind]) -> bool {
        if let Some(&kind) = kinds.first() {
            self.expected_tokens.insert(kind);
        }
        let mut rest = self.rest();
        kinds
            .iter()
            .all(|kind| rest.next().is_some_and(|token| token.kind == *kind))
            && rest.next().is_some_and(|token| {
                matches!(
                    token.kind,
                    TokenKind::Comment | TokenKind::Eof | TokenKind::Eol
                )
            })
    }

    fn advance(&mut self) -> Result<Token<'src>> {
        self.expected_tokens.clear();
        while let Some(&token) = self.tokens.get(self.next_token) {
            self.next_token += 1;
            if token.kind != TokenKind::Whitespace {
                return Ok(token);
            }
        }
        Err(self.internal_error("parser advanced past end of token stream"))
    }

    fn expect(&mut self, expected: TokenKind) -> Result<Token<'src>> {
        match self.accept(expected)? {
            Some(token) => Ok(token),
            None => Err(self.unexpected_token()),
        }
    }

    fn expect_any(&mut self, expected: &[TokenKind]) -> Result<Token<'src>> {
        for &kind in expected {
            if let Some(token) = self.accept(kind)? {
                return Ok(token);
            }
        }
        Err(self.unexpected_token())
    }

    fn expect_eol(&mut self) -> Result {
        self.accept(TokenKind::Comment)?;
        if self.next_is(TokenKind::Eof) {
            return Ok(());
        }
        self.expect(TokenKind::Eol).map(|_| ())
    }

    fn expect_keyword(&mut self, expected: &str) -> Result {
        let found = self.advance()?;
        if found.kind == TokenKind::Identifier && found.lexeme == expected {
            Ok(())
        } else if found.kind == TokenKind::Identifier {
            Err(found.error(format!(
                "expected keyword `{expected}` but found identifier `{}`",
                found.lexeme
            )))
        } else {
            Err(found.error(format!(
                "expected keyword `{expected}` but found {}",
                found.kind
            )))
        }
    }

    fn presume_keyword(&mut self, keyword: &str) -> Result<Token<'src>> {
        let next = self.advance()?;
        if next.kind == TokenKind::Identifier && next.lexeme == keyword {
            Ok(next)
        } else {
            Err(next.error(format!(
                "internal error: presumed keyword `{keyword}` but found `{}`",
                next.lexeme
            )))
        }
    }

    fn presume(&mut self, kind: TokenKind) -> Result<Token<'src>> {
        let next = self.advance()?;
        if next.kind == kind {
            Ok(next)
        } else {
            Err(next.error(format!(
                "internal error: presumed {kind} but found {}",
                next.kind
            )))
        }
    }

    fn accept(&mut self, kind: TokenKind) -> Result<Option<Token<'src>>> {
        if self.next_is(kind) {
            Ok(Some(self.advance()?))
        } else {
            Ok(None)
        }
    }

    fn accepted(&mut self, kind: TokenKind) -> Result<bool> {
        Ok(self.accept(kind)?.is_some())
    }

    fn accept_keyword(&mut self, keyword: &str) -> Result<Option<Token<'src>>> {
        let next = self.next()?;
        if next.kind == TokenKind::Identifier && next.lexeme == keyword {
            self.advance()?;
            Ok(Some(next))
        } else {
            Ok(None)
        }
    }

    fn accept_dependency(&mut self) -> Result<Option<Dependency<'src>>> {
        if self.next_is(TokenKind::Identifier) {
            return Ok(Some(Dependency {
                arguments: Vec::new(),
                recipe: self.parse_namepath()?,
            }));
        }

        if !(self.next_is(TokenKind::Asterisk) || self.next_is(TokenKind::ParenL)) {
            return Ok(None);
        }

        let star = self.accept(TokenKind::Asterisk)?;

        self.expect(TokenKind::ParenL)?;

        let recipe = self.parse_namepath()?;

        let mut arguments = Vec::new();
        let mut starred_argument = None;

        while !self.accepted(TokenKind::ParenR)? {
            let token = self.accept(TokenKind::Asterisk)?;

            if let Some(token) = token {
                if starred_argument.is_some() {
                    return Err(
                        token.error("mapped dependencies may not have multiple starred arguments")
                    );
                }
                starred_argument = Some(token);
            }

            let expression = if token.is_some() {
                *self.parse_value()?
            } else {
                self.parse_expression()?
            };

            arguments.push(DependencyArgument {
                expression,
                starred: token.is_some(),
            });
        }

        match (star, starred_argument) {
            (None, None) | (Some(_), Some(_)) => Ok(Some(Dependency { arguments, recipe })),
            (Some(star), None) => Err(star.error("mapped dependencies must have starred argument")),
            (None, Some(starred)) => {
                Err(starred.error("starred argument outside of mapped dependency"))
            }
        }
    }

    fn take_doc_comment(&mut self, attributes: &AttributeSet) -> Option<String> {
        if contains(attributes, AttributeKind::Doc) {
            return None;
        }

        let mut items = self.items.iter().rev();

        if !matches!(items.next()?, Item::Newline) {
            return None;
        }

        let Item::Comment(contents) = items.next()? else {
            return None;
        };

        let first = match items.next() {
            None => true,
            Some(Item::Newline) => false,
            Some(_) => return None,
        };

        if first && contents.starts_with("#!") {
            return None;
        }

        let doc = contents.get(1..)?.trim().to_string();

        if doc.is_empty() {
            return None;
        }

        self.items.pop();
        self.items.pop();

        Some(doc)
    }

    fn parse_ast(mut self) -> Result<Vec<Item<'src>>> {
        self.accept(TokenKind::ByteOrderMark)?;

        while !self.accepted(TokenKind::Eof)? {
            let mut attributes = self.parse_attributes()?;

            let item = self.parse_item(&mut attributes)?;

            let unterminated = match &item {
                Item::Newline => false,
                Item::Recipe(recipe) => recipe.body.is_empty(),
                _ => true,
            };

            let terminated_recipe =
                matches!(&item, Item::Recipe(recipe) if !recipe.body.is_empty());

            self.items.push(item);

            if terminated_recipe {
                self.items.push(Item::Newline);
            }

            if let Some((token, attributes)) = attributes {
                return Err(token.error(format!(
                    "extraneous attribute{}",
                    if attributes.len() == 1 { "" } else { "s" }
                )));
            }

            if unterminated {
                if let Some(comment) = self.accept(TokenKind::Comment)? {
                    self.items.push(Item::Comment(comment.lexeme.trim_end()));
                }

                if !self.next_is(TokenKind::Eof) {
                    self.expect(TokenKind::Eol)?;
                    self.items.push(Item::Newline);
                }
            }
        }

        if self.next_token != self.tokens.len() {
            return Err(self.internal_error("parse completed with unparsed tokens"));
        }

        Ok(self.items)
    }

    fn parse_item(
        &mut self,
        attributes: &mut Option<(Token<'src>, AttributeSet<'src>)>,
    ) -> Result<Item<'src>> {
        use TokenKind::{ColonEquals, Identifier, ParenL, QuestionMark, StringToken};

        if let Some(comment) = self.accept(TokenKind::Comment)? {
            return Ok(Item::Comment(comment.lexeme.trim_end()));
        }

        if self.accepted(TokenKind::Eol)? {
            return Ok(Item::Newline);
        }

        if self.accepted(TokenKind::At)? {
            return Ok(Item::Recipe(Box::new(
                self.parse_recipe(take_attributes(attributes), true)?,
            )));
        }

        if !self.next_is(Identifier) {
            return Err(self.unexpected_token());
        }

        let keyword = self.next()?.lexeme;

        if keyword == "alias" && self.next_are(&[Identifier, Identifier, ColonEquals]) {
            self.presume_keyword("alias")?;
            let name = self.parse_name()?;
            self.presume(ColonEquals)?;
            let target = self.parse_namepath()?;
            let attributes = take_attributes(attributes);
            ensure_valid_attributes(&attributes, "alias", &[AttributeKind::Private], name)?;
            return Ok(Item::Alias {
                attributes,
                name,
                target,
            });
        }

        if keyword == "eager" && self.next_are(&[Identifier, Identifier, ColonEquals]) {
            self.presume_keyword("eager")?;
            return self.parse_assignment(take_attributes(attributes), true, false);
        }

        if keyword == "export" && self.next_are(&[Identifier, Identifier, ColonEquals]) {
            self.presume_keyword("export")?;
            return self.parse_assignment(take_attributes(attributes), false, true);
        }

        if keyword == "unexport" && self.line_is(&[Identifier, Identifier]) {
            self.presume_keyword("unexport")?;
            let name = self.parse_name()?;
            let attributes = take_attributes(attributes);
            ensure_valid_attributes(&attributes, "unexport", &[], name)?;
            return Ok(Item::Unexport { attributes, name });
        }

        if keyword == "import"
            && (self.next_are(&[Identifier, Identifier, StringToken])
                || self.next_are(&[Identifier, QuestionMark])
                || self.next_are(&[Identifier, StringToken]))
        {
            self.presume_keyword("import")?;
            let optional = self.accepted(QuestionMark)?;
            let relative = self.parse_string_literal()?;
            let attributes = take_attributes(attributes);
            ensure_valid_attributes(&attributes, "import", &[], relative.token)?;
            return Ok(Item::Import {
                attributes,
                optional,
                relative,
            });
        }

        if keyword == "mod"
            && (self.line_is(&[Identifier, Identifier])
                || self.next_are(&[Identifier, Identifier, Identifier, StringToken])
                || self.next_are(&[Identifier, Identifier, StringToken])
                || self.next_are(&[Identifier, QuestionMark]))
        {
            self.presume_keyword("mod")?;
            let optional = self.accepted(QuestionMark)?;
            let name = self.parse_name()?;
            let relative = if self.next_is(StringToken) || self.next_are(&[Identifier, StringToken])
            {
                Some(self.parse_string_literal()?)
            } else {
                None
            };
            let attributes = take_attributes(attributes);
            ensure_valid_attributes(
                &attributes,
                "module",
                &[
                    AttributeKind::Doc,
                    AttributeKind::Group,
                    AttributeKind::Private,
                ],
                name,
            )?;
            let doc = self.take_doc_comment(&attributes);
            return Ok(Item::Module {
                attributes,
                doc,
                name,
                optional,
                relative,
            });
        }

        if keyword == "set"
            && (self.next_are(&[Identifier, Identifier, ColonEquals])
                || self.line_is(&[Identifier, Identifier]))
        {
            return self.parse_set(take_attributes(attributes));
        }

        if self.next_are(&[Identifier, ParenL]) {
            return self.parse_function_definition(take_attributes(attributes));
        }

        if self.next_are(&[Identifier, ColonEquals]) {
            return self.parse_assignment(take_attributes(attributes), false, false);
        }

        Ok(Item::Recipe(Box::new(
            self.parse_recipe(take_attributes(attributes), false)?,
        )))
    }

    fn parse_function_definition(&mut self, attributes: AttributeSet<'src>) -> Result<Item<'src>> {
        let name = self.parse_name()?;

        ensure_valid_attributes(&attributes, "function", &[], name)?;

        self.presume(TokenKind::ParenL)?;

        let mut parameters = Vec::new();
        while !self.next_is(TokenKind::ParenR) {
            parameters.push(self.parse_name()?);
            if !self.accepted(TokenKind::Comma)? {
                break;
            }
        }

        self.expect(TokenKind::ParenR)?;

        self.expect(TokenKind::ColonEquals)?;

        let body = self.parse_expression()?;

        Ok(Item::Function {
            attributes,
            name,
            parameters,
            body,
        })
    }

    fn parse_assignment(
        &mut self,
        attributes: AttributeSet<'src>,
        eager: bool,
        export: bool,
    ) -> Result<Item<'src>> {
        let name = self.parse_name()?;
        self.presume(TokenKind::ColonEquals)?;
        let value = self.parse_expression()?;

        ensure_valid_attributes(&attributes, "assignment", &[AttributeKind::Private], name)?;

        Ok(Item::Assignment {
            attributes,
            eager,
            export,
            name,
            value,
        })
    }

    fn enter(&mut self) -> Result {
        if self.nesting_depth >= NESTING_LIMIT {
            return Err(self.next()?.error("expression nesting too deep"));
        }
        self.nesting_depth += 1;
        Ok(())
    }

    fn parse_expression(&mut self) -> Result<Expression<'src>> {
        Ok(*self.parse_boxed_expression()?)
    }

    fn parse_boxed_expression(&mut self) -> Result<Box<Expression<'src>>> {
        if self.recursion_depth == RECURSION_LIMIT {
            return Err(self.next()?.error("parsing recursion depth exceeded"));
        }

        self.recursion_depth += 1;

        let disjunct = self.parse_disjunct()?;

        let expression = if self.accepted(TokenKind::BarBar)? {
            let rhs = self.parse_boxed_expression()?;
            Box::new(Expression::Or { lhs: disjunct, rhs })
        } else {
            disjunct
        };

        self.recursion_depth -= 1;

        Ok(expression)
    }

    fn parse_disjunct(&mut self) -> Result<Box<Expression<'src>>> {
        self.enter()?;

        let conjunct = self.parse_comparison()?;

        let expression = if self.accepted(TokenKind::AmpersandAmpersand)? {
            let rhs = self.parse_disjunct()?;
            Box::new(Expression::And { lhs: conjunct, rhs })
        } else {
            conjunct
        };

        self.nesting_depth -= 1;

        Ok(expression)
    }

    fn parse_comparison(&mut self) -> Result<Box<Expression<'src>>> {
        let lhs = self.parse_conjunct()?;

        let (token, operator) = if let Some(token) = self.accept(TokenKind::BangEquals)? {
            (token, ConditionalOperator::Inequality)
        } else if let Some(token) = self.accept(TokenKind::EqualsTilde)? {
            (token, ConditionalOperator::RegexMatch)
        } else if let Some(token) = self.accept(TokenKind::BangTilde)? {
            (token, ConditionalOperator::RegexMismatch)
        } else if let Some(token) = self.accept(TokenKind::EqualsEquals)? {
            (token, ConditionalOperator::Equality)
        } else {
            return Ok(lhs);
        };

        let rhs = self.parse_conjunct()?;

        Ok(Box::new(Expression::Comparison {
            lhs,
            operator,
            rhs,
            token,
        }))
    }

    fn parse_conjunct(&mut self) -> Result<Box<Expression<'src>>> {
        self.enter()?;

        let expression = if self.next_is_keyword("if") {
            self.parse_conditional()?
        } else if let Some(operator) = self.accept(TokenKind::Slash)? {
            let rhs = self.parse_conjunct()?;
            Box::new(Expression::Join {
                lhs: None,
                operator,
                rhs,
            })
        } else {
            let value = self.parse_value()?;

            if let Some(operator) = self.accept(TokenKind::Slash)? {
                let rhs = self.parse_conjunct()?;
                Box::new(Expression::Join {
                    lhs: Some(value),
                    operator,
                    rhs,
                })
            } else if let Some(operator) = self.accept(TokenKind::PlusPlus)? {
                let rhs = self.parse_conjunct()?;
                Box::new(Expression::ListConcatenation {
                    lhs: value,
                    operator,
                    rhs,
                })
            } else if let Some(operator) = self.accept(TokenKind::Plus)? {
                let rhs = self.parse_conjunct()?;
                Box::new(Expression::Concatenation {
                    lhs: value,
                    operator,
                    rhs,
                })
            } else {
                value
            }
        };

        self.nesting_depth -= 1;

        Ok(expression)
    }

    #[inline(never)]
    fn parse_conditional(&mut self) -> Result<Box<Expression<'src>>> {
        self.enter()?;

        self.presume_keyword("if")?;

        let condition = self.parse_boxed_expression()?;

        self.expect(TokenKind::BraceL)?;

        let then = self.parse_boxed_expression()?;

        self.expect(TokenKind::BraceR)?;

        let otherwise = if self.accept_keyword("else")?.is_some() {
            if self.next_is_keyword("if") {
                Some(self.parse_conditional()?)
            } else {
                self.expect(TokenKind::BraceL)?;
                let otherwise = self.parse_boxed_expression()?;
                self.expect(TokenKind::BraceR)?;
                Some(otherwise)
            }
        } else {
            None
        };

        self.nesting_depth -= 1;

        Ok(Box::new(Expression::Conditional {
            condition,
            then,
            otherwise,
        }))
    }

    #[inline(never)]
    fn parse_format_string(&mut self) -> Result<Box<Expression<'src>>> {
        self.presume_keyword("f")?;

        let start = self.parse_string_literal_in_state(StringState::FormatStart)?;

        let Some(kind) = StringKind::from_token_start(start.token.lexeme) else {
            return Err(start.token.error("internal error: expected string"));
        };

        let mut more = start.token.kind == TokenKind::FormatStringStart;

        let mut expressions = Vec::new();

        while more {
            let expression = self.parse_expression()?;
            more = self.next_is(TokenKind::FormatStringContinue);
            expressions.push((
                expression,
                self.parse_string_literal_in_state(StringState::FormatContinue(kind))?,
            ));
        }

        Ok(Box::new(Expression::FormatString { start, expressions }))
    }

    fn next_is_prefixed_string(&self, prefix: &str, kinds: &[TokenKind]) -> bool {
        let mut tokens = self.tokens[self.next_token.min(self.tokens.len())..]
            .iter()
            .skip_while(|token| token.kind == TokenKind::Whitespace);

        tokens
            .next()
            .is_some_and(|token| token.kind == TokenKind::Identifier && token.lexeme == prefix)
            && tokens
                .next()
                .is_some_and(|token| kinds.contains(&token.kind))
    }

    fn next_is_shell_expanded_string(&self) -> bool {
        self.next_is_prefixed_string("x", &[TokenKind::StringToken])
    }

    fn next_is_format_string(&self) -> bool {
        self.next_is_prefixed_string("f", &[TokenKind::StringToken, TokenKind::FormatStringStart])
    }

    fn parse_value(&mut self) -> Result<Box<Expression<'src>>> {
        self.enter()?;

        let value = if self.accepted(TokenKind::Bang)? {
            let operand = self.parse_value()?;
            Box::new(Expression::Not { operand })
        } else if self.next_is(TokenKind::StringToken) || self.next_is_shell_expanded_string() {
            self.parse_string_literal_value()?
        } else if self.next_is_format_string() {
            self.parse_format_string()?
        } else if self.next_is(TokenKind::Backtick) {
            self.parse_backtick()?
        } else if self.next_is(TokenKind::Identifier) {
            self.parse_identifier_value()?
        } else if self.next_is(TokenKind::ParenL) {
            self.parse_group()?
        } else if self.next_is(TokenKind::BracketL) {
            self.parse_list()?
        } else {
            return Err(self.unexpected_token());
        };

        self.nesting_depth -= 1;

        Ok(value)
    }

    #[inline(never)]
    fn parse_string_literal_value(&mut self) -> Result<Box<Expression<'src>>> {
        let string_literal = self.parse_string_literal()?;
        Ok(Box::new(Expression::StringLiteral { string_literal }))
    }

    #[inline(never)]
    fn parse_backtick(&mut self) -> Result<Box<Expression<'src>>> {
        let token = self.advance()?;
        let Some(kind) = StringKind::from_token_start(token.lexeme) else {
            return Err(token.error("internal error: expected backtick"));
        };
        let delimiter = kind.delimiter().len();
        let Some(contents) = token
            .lexeme
            .len()
            .checked_sub(delimiter)
            .and_then(|end| token.lexeme.get(delimiter..end))
        else {
            return Err(token.error("internal error: malformed backtick"));
        };
        let contents = if kind.indented {
            unindent(contents)
        } else {
            contents.to_string()
        };
        if contents.starts_with("#!") {
            return Err(token.error("backticks may not start with `#!`"));
        }
        Ok(Box::new(Expression::Backtick { contents, token }))
    }

    #[inline(never)]
    fn parse_identifier_value(&mut self) -> Result<Box<Expression<'src>>> {
        if let Some(name) = self.accept_keyword("assert")? {
            self.expect(TokenKind::ParenL)?;
            let condition = self.parse_boxed_expression()?;
            let message = if self.accepted(TokenKind::Comma)? {
                Some(self.parse_boxed_expression()?)
            } else {
                None
            };
            self.expect(TokenKind::ParenR)?;
            return Ok(Box::new(Expression::Assert {
                condition,
                message,
                name,
            }));
        }

        let name = self.parse_name()?;

        if self.next_is(TokenKind::ParenL) {
            let arguments = self.parse_sequence()?;
            return Ok(Box::new(Expression::Call { name, arguments }));
        }

        Ok(Box::new(Expression::Variable { name }))
    }

    #[inline(never)]
    fn parse_group(&mut self) -> Result<Box<Expression<'src>>> {
        self.presume(TokenKind::ParenL)?;
        let contents = self.parse_boxed_expression()?;
        self.expect(TokenKind::ParenR)?;
        Ok(Box::new(Expression::Group { contents }))
    }

    #[inline(never)]
    fn parse_list(&mut self) -> Result<Box<Expression<'src>>> {
        let open = self.presume(TokenKind::BracketL)?;

        let mut elements = Vec::new();

        while !self.next_is(TokenKind::BracketR) {
            elements.push(self.parse_expression()?);
            if !self.accepted(TokenKind::Comma)? {
                break;
            }
        }

        self.expect(TokenKind::BracketR)?;

        Ok(Box::new(Expression::List { elements, open }))
    }

    fn parse_string_literal(&mut self) -> Result<StringLiteral<'src>> {
        self.parse_string_literal_in_state(StringState::Normal)
    }

    #[inline(never)]
    fn parse_string_literal_in_state(&mut self, state: StringState) -> Result<StringLiteral<'src>> {
        let normal = matches!(state, StringState::Normal);

        let expand = if normal && self.next_is(TokenKind::Identifier) {
            self.expect_keyword("x")?;
            true
        } else {
            false
        };

        let token = match state {
            StringState::Normal => self.expect(TokenKind::StringToken)?,
            StringState::FormatStart => {
                self.expect_any(&[TokenKind::StringToken, TokenKind::FormatStringStart])?
            }
            StringState::FormatContinue(_) => {
                self.expect_any(&[TokenKind::FormatStringContinue, TokenKind::FormatStringEnd])?
            }
        };

        let kind = match state {
            StringState::Normal | StringState::FormatStart => {
                match StringKind::from_token_start(token.lexeme) {
                    Some(kind) => kind,
                    None => return Err(token.error("internal error: expected string")),
                }
            }
            StringState::FormatContinue(kind) => kind,
        };

        let open = if matches!(
            token.kind,
            TokenKind::FormatStringContinue | TokenKind::FormatStringEnd
        ) {
            INTERPOLATION_END.len()
        } else {
            kind.delimiter().len()
        };

        let close = if matches!(
            token.kind,
            TokenKind::FormatStringStart | TokenKind::FormatStringContinue
        ) {
            INTERPOLATION_START.len()
        } else {
            kind.delimiter().len()
        };

        let Some(raw) = token
            .lexeme
            .len()
            .checked_sub(close)
            .and_then(|end| token.lexeme.get(open..end))
        else {
            return Err(token.error("internal error: malformed string"));
        };

        let unindented = if kind.indented && normal {
            unindent(raw)
        } else {
            raw.to_string()
        };

        let undelimited = if normal {
            unindented
        } else {
            unindented.replace(INTERPOLATION_ESCAPE, INTERPOLATION_START)
        };

        let cooked = if kind.processes_escape_sequences() {
            cook_string(token, &undelimited)?
        } else {
            undelimited
        };

        let part = match token.kind {
            TokenKind::FormatStringStart => Some(FormatStringPart::Start),
            TokenKind::FormatStringContinue => Some(FormatStringPart::Continue),
            TokenKind::FormatStringEnd => Some(FormatStringPart::End),
            TokenKind::StringToken => {
                if normal {
                    None
                } else {
                    Some(FormatStringPart::Single)
                }
            }
            _ => return Err(token.error("internal error: unexpected string token")),
        };

        Ok(StringLiteral {
            cooked,
            expand,
            kind,
            part,
            token,
        })
    }

    fn parse_name(&mut self) -> Result<Token<'src>> {
        self.expect(TokenKind::Identifier)
    }

    fn parse_namepath(&mut self) -> Result<Vec<Token<'src>>> {
        let mut path = vec![self.parse_name()?];
        while self.accepted(TokenKind::ColonColon)? {
            path.push(self.parse_name()?);
        }
        Ok(path)
    }

    #[inline(never)]
    fn parse_sequence(&mut self) -> Result<Vec<Expression<'src>>> {
        self.presume(TokenKind::ParenL)?;

        let mut elements = Vec::new();

        while !self.next_is(TokenKind::ParenR) {
            elements.push(self.parse_expression()?);
            if !self.accepted(TokenKind::Comma)? {
                break;
            }
        }

        self.expect(TokenKind::ParenR)?;

        Ok(elements)
    }

    fn parse_recipe(
        &mut self,
        attributes: AttributeSet<'src>,
        quiet: bool,
    ) -> Result<Recipe<'src>> {
        let name = self.parse_name()?;

        let mut longs = HashSet::new();
        let mut shorts = HashSet::new();
        let mut arg_attributes = BTreeMap::new();

        for attribute in &attributes {
            let Attribute::Arg(arg) = attribute else {
                continue;
            };

            if let Some(option) = &arg.long
                && !longs.insert(option.cooked.clone())
            {
                return Err(arg.long_key.unwrap_or(option.token).error(format!(
                    "recipe `{}` defines option `--{}` multiple times",
                    name.lexeme, option.cooked
                )));
            }

            if let Some(option) = &arg.short
                && let Some(short) = option.cooked.chars().next()
                && !shorts.insert(short)
            {
                return Err(arg.short_key.unwrap_or(option.token).error(format!(
                    "recipe `{}` defines option `-{short}` multiple times",
                    name.lexeme
                )));
            }

            arg_attributes.insert(
                arg.name.cooked.clone(),
                ArgInfo {
                    flag: arg.flag.is_some(),
                    multiple: arg.multiple.is_some(),
                    count_key: arg.max_key.or(arg.min_key),
                    name: arg.name.token,
                },
            );
        }

        let mut parameters = Vec::new();

        while self.next_is(TokenKind::Identifier) || self.next_is(TokenKind::Dollar) {
            parameters.push(self.parse_parameter(&mut arg_attributes, ParameterKind::Singular)?);
        }

        let kind = if self.accepted(TokenKind::Plus)? {
            ParameterKind::Plus
        } else if self.accepted(TokenKind::Asterisk)? {
            ParameterKind::Star
        } else {
            ParameterKind::Singular
        };

        if kind != ParameterKind::Singular {
            parameters.push(self.parse_parameter(&mut arg_attributes, kind)?);

            let next = self.next()?;
            if next.kind == TokenKind::Identifier {
                return Err(next.error(format!(
                    "parameter `{}` follows variadic parameter",
                    next.lexeme
                )));
            }
        }

        self.expect(TokenKind::Colon)?;

        if let Some((argument, info)) = arg_attributes.pop_first() {
            return Err(info.name.error(format!(
                "argument attribute for undefined argument `{argument}`"
            )));
        }

        let mut dependencies = Vec::new();

        while let Some(dependency) = self.accept_dependency()? {
            dependencies.push(dependency);
        }

        let priors = dependencies.len();

        if self.accepted(TokenKind::AmpersandAmpersand)? {
            let mut subsequents = Vec::new();

            while let Some(subsequent) = self.accept_dependency()? {
                subsequents.push(subsequent);
            }

            if subsequents.is_empty() {
                return Err(self.unexpected_token());
            }

            dependencies.append(&mut subsequents);
        }

        if self.next_are(&[TokenKind::Comment, TokenKind::Eol, TokenKind::Indent])
            || self.next_are(&[TokenKind::Eol, TokenKind::Indent])
        {
            self.expect_eol()?;
        }

        let body = self.parse_body()?;

        let both = |first: AttributeKind, second: AttributeKind| {
            contains(&attributes, first) && contains(&attributes, second)
        };

        if both(AttributeKind::WorkingDirectory, AttributeKind::NoCd) {
            return Err(name.error(format!(
                "recipe `{}` has both `[no-cd]` and `[working-directory]` attributes",
                name.lexeme
            )));
        }

        if both(AttributeKind::ExitMessage, AttributeKind::NoExitMessage) {
            return Err(name.error(format!(
                "recipe `{}` has both `[exit-message]` and `[no-exit-message]` attributes",
                name.lexeme
            )));
        }

        if both(AttributeKind::Script, AttributeKind::Shell) {
            return Err(name.error(format!(
                "recipe `{}` has both `[script]` and `[shell]` attributes",
                name.lexeme
            )));
        }

        let doc = self.take_doc_comment(&attributes);

        Ok(Recipe {
            attributes,
            body,
            dependencies,
            doc,
            name,
            parameters,
            priors,
            quiet,
        })
    }

    fn parse_parameter(
        &mut self,
        arg_attributes: &mut BTreeMap<String, ArgInfo<'src>>,
        kind: ParameterKind,
    ) -> Result<Parameter<'src>> {
        let export = self.accepted(TokenKind::Dollar)?;

        let name = self.parse_name()?;

        let default = if self.accepted(TokenKind::Equals)? {
            Some(*self.parse_value()?)
        } else {
            None
        };

        if let Some(arg) = arg_attributes.remove(name.lexeme) {
            if arg.flag && default.is_some() {
                return Err(name.error(format!(
                    "flag parameter `{}` may not have a default",
                    name.lexeme
                )));
            }

            if let Some(key) = arg.count_key
                && !arg.multiple
                && kind == ParameterKind::Singular
            {
                return Err(key.error(format!(
                    "argument attribute `{}` only valid with `multiple` or a variadic parameter",
                    key.lexeme
                )));
            }
        }

        Ok(Parameter {
            default,
            export,
            kind,
            name,
        })
    }

    fn parse_body(&mut self) -> Result<Vec<Vec<Fragment<'src>>>> {
        let mut lines = Vec::new();

        if self.accepted(TokenKind::Indent)? {
            while !self.accepted(TokenKind::Dedent)? {
                let mut fragments = Vec::new();

                if !self.accepted(TokenKind::Eol)? {
                    while !(self.accepted(TokenKind::Eol)? || self.next_is(TokenKind::Dedent)) {
                        if let Some(token) = self.accept(TokenKind::Text)? {
                            fragments.push(Fragment::Text(token));
                        } else if self.accepted(TokenKind::InterpolationStart)? {
                            fragments.push(Fragment::Interpolation(self.parse_expression()?));
                            self.expect(TokenKind::InterpolationEnd)?;
                        } else {
                            return Err(self.unexpected_token());
                        }
                    }
                }

                lines.push(fragments);
            }
        }

        while lines.last().is_some_and(Vec::is_empty) {
            lines.pop();
        }

        Ok(lines)
    }

    fn parse_set_bool(&mut self) -> Result<bool> {
        if !self.accepted(TokenKind::ColonEquals)? {
            return Ok(true);
        }

        let identifier = self.expect(TokenKind::Identifier)?;

        match identifier.lexeme {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(identifier.error(format!(
                "expected keyword `true` or `false` but found identifier `{}`",
                identifier.lexeme
            ))),
        }
    }

    fn parse_setting_string(&mut self, name: Token<'src>) -> Result<StringLiteral<'src>> {
        match self.parse_expression()? {
            Expression::StringLiteral { string_literal }
                if !string_literal.expand
                    && !string_literal.kind.indented
                    && string_literal.part.is_none() =>
            {
                Ok(string_literal)
            }
            _ => Err(name.error(format!(
                "setting `{}` value must be a string literal",
                name.lexeme
            ))),
        }
    }

    fn parse_set(&mut self, attributes: AttributeSet<'src>) -> Result<Item<'src>> {
        self.presume_keyword("set")?;
        let name = self.presume(TokenKind::Identifier)?;
        let setting = name.lexeme;

        let known = BOOLEAN_SETTINGS.contains(&setting)
            || EXPRESSION_SETTINGS.contains(&setting)
            || INTERPRETER_SETTINGS.contains(&setting)
            || setting == "indentation"
            || setting == "minimum-version";

        if !known {
            return Err(name.error(format!("unknown setting `{setting}`")));
        }

        ensure_valid_attributes(&attributes, "setting", &[], name)?;

        if BOOLEAN_SETTINGS.contains(&setting) {
            let value = Setting::Bool(self.parse_set_bool()?);
            return Ok(Item::Setting {
                attributes,
                name,
                value,
            });
        }

        self.expect(TokenKind::ColonEquals)?;

        let value = if EXPRESSION_SETTINGS.contains(&setting) {
            Setting::Expression(self.parse_expression()?)
        } else if INTERPRETER_SETTINGS.contains(&setting) {
            Setting::Interpreter(self.parse_interpreter()?)
        } else if setting == "indentation" {
            let string_literal = self.parse_setting_string(name)?;
            let indentation = parse_indentation(&string_literal.cooked)
                .map_err(|message| string_literal.token.error(message))?;
            Setting::Indentation(Expression::StringLiteral { string_literal }, indentation)
        } else if setting == "minimum-version" {
            let string_literal = self.parse_setting_string(name)?;
            let Some(minimum) = parse_version(&string_literal.cooked) else {
                return Err(string_literal.token.error(format!(
                    "`minimum-version` setting has invalid version `{}`: expected `MAJOR.MINOR.PATCH`",
                    string_literal.cooked
                )));
            };
            if CURRENT_VERSION < minimum {
                return Err(string_literal.token.error(format!(
                    "justfile requires just {} or later, but using {}.{}.{}",
                    string_literal.cooked, CURRENT_VERSION.0, CURRENT_VERSION.1, CURRENT_VERSION.2
                )));
            }
            Setting::Expression(Expression::StringLiteral { string_literal })
        } else {
            return Err(name.error(format!("unknown setting `{setting}`")));
        };

        Ok(Item::Setting {
            attributes,
            name,
            value,
        })
    }

    fn parse_interpreter(&mut self) -> Result<Interpreter<Expression<'src>>> {
        self.expect(TokenKind::BracketL)?;

        let command = self.parse_expression()?;

        let mut arguments = Vec::new();

        if self.accepted(TokenKind::Comma)? {
            while !self.next_is(TokenKind::BracketR) {
                arguments.push(self.parse_expression()?);
                if !self.accepted(TokenKind::Comma)? {
                    break;
                }
            }
        }

        self.expect(TokenKind::BracketR)?;

        Ok(Interpreter { command, arguments })
    }

    fn parse_attributes(&mut self) -> Result<Option<(Token<'src>, AttributeSet<'src>)>> {
        let mut arg_attributes = BTreeMap::new();
        let mut group_attributes = BTreeMap::new();
        let mut attributes = AttributeSet::new();
        let mut kinds = BTreeMap::new();

        let mut first_bracket = None;

        while let Some(bracket) = self.accept(TokenKind::BracketL)? {
            first_bracket.get_or_insert(bracket);

            loop {
                let name = self.parse_name()?;

                let Some(kind) = AttributeKind::from_name(name.lexeme) else {
                    return Err(name.error(format!("unknown attribute `{}`", name.lexeme)));
                };

                let mut arguments = Vec::new();
                let mut keyword_arguments = KeywordArguments::new();

                if self.accepted(TokenKind::Colon)? {
                    let token = self.next()?;
                    let expression = self.parse_expression()?;
                    arguments.push((token, expression));
                } else if self.accepted(TokenKind::ParenL)? {
                    if !self.next_is(TokenKind::ParenR) {
                        loop {
                            if kind.accepts_keyword_arguments()
                                && self.next_is(TokenKind::Identifier)
                                && !self.next_is_shell_expanded_string()
                            {
                                let key = self.parse_name()?;

                                let value = if self.accepted(TokenKind::Equals)? {
                                    Some(self.parse_expression()?)
                                } else {
                                    None
                                };

                                if keyword_arguments.insert(key.lexeme, (key, value)).is_some() {
                                    return Err(key.error(format!(
                                        "duplicate key `{}` for `{}` attribute",
                                        key.lexeme, name.lexeme
                                    )));
                                }
                            } else {
                                let token = self.next()?;
                                let expression = self.parse_expression()?;

                                if !keyword_arguments.is_empty() {
                                    return Err(token.error(
                                        "positional attribute arguments cannot follow keyword attribute arguments",
                                    ));
                                }

                                arguments.push((token, expression));
                            }

                            if !self.accepted(TokenKind::Comma)? || self.next_is(TokenKind::ParenR)
                            {
                                break;
                            }
                        }
                    }

                    self.expect(TokenKind::ParenR)?;
                }

                let attribute = new_attribute(name, kind, arguments, keyword_arguments)?;

                if !attribute.repeatable()
                    && let Some(first) = kinds.get(&attribute.kind())
                {
                    return Err(name.error(format!(
                        "attribute `{}` first used on line {} is duplicated on line {}",
                        name.lexeme,
                        first + 1,
                        name.line + 1
                    )));
                }

                if let Attribute::Arg(arg) = &attribute {
                    if let Some(first) = arg_attributes.get(&arg.name.cooked) {
                        return Err(name.error(format!(
                            "attribute for argument `{}` first used on line {} is duplicated on line {}",
                            arg.name.cooked,
                            first + 1,
                            name.line + 1
                        )));
                    }
                    arg_attributes.insert(arg.name.cooked.clone(), name.line);
                }

                if let Attribute::Group(group) = &attribute {
                    if let Some(first) = group_attributes.get(&group.cooked) {
                        return Err(name.error(format!(
                            "`[group({group})]` attribute first used on line {} is duplicated on line {}",
                            first + 1,
                            name.line + 1
                        )));
                    }
                    group_attributes.insert(group.cooked.clone(), name.line);
                }

                kinds.insert(attribute.kind(), name.line);

                attributes.insert(attribute);

                if !self.accepted(TokenKind::Comma)? {
                    break;
                }
            }

            self.expect(TokenKind::BracketR)?;
            self.expect_eol()?;
        }

        match first_bracket {
            Some(bracket) if !attributes.is_empty() => Ok(Some((bracket, attributes))),
            _ => Ok(None),
        }
    }
}
