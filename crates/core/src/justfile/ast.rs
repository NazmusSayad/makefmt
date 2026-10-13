use std::collections::BTreeSet;
use std::fmt::{self, Display, Formatter, Write};

use super::lexer::{StringKind, Token};

#[derive(PartialEq, Eq, Debug, Copy, Clone, Ord, PartialOrd)]
pub enum ConditionalOperator {
    Equality,
    Inequality,
    RegexMatch,
    RegexMismatch,
}

impl Display for ConditionalOperator {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Equality => write!(f, "=="),
            Self::Inequality => write!(f, "!="),
            Self::RegexMatch => write!(f, "=~"),
            Self::RegexMismatch => write!(f, "!~"),
        }
    }
}

#[derive(PartialEq, Debug, Clone, Copy, Ord, Eq, PartialOrd)]
pub enum FormatStringPart {
    Continue,
    End,
    Single,
    Start,
}

#[derive(PartialEq, Debug, Clone, Ord, Eq, PartialOrd)]
pub struct StringLiteral<'src> {
    pub cooked: String,
    pub expand: bool,
    pub kind: StringKind,
    pub part: Option<FormatStringPart>,
    pub token: Token<'src>,
}

impl Display for StringLiteral<'_> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        if self.expand {
            write!(f, "x")?;
        }
        if let Some(FormatStringPart::Start | FormatStringPart::Single) = self.part {
            write!(f, "f")?;
        }
        write!(f, "{}", self.token.lexeme)
    }
}

#[derive(PartialEq, Eq, Debug, Clone, Ord, PartialOrd)]
pub enum Expression<'src> {
    And {
        lhs: Box<Self>,
        rhs: Box<Self>,
    },
    Assert {
        condition: Box<Self>,
        message: Option<Box<Self>>,
        name: Token<'src>,
    },
    Backtick {
        contents: String,
        token: Token<'src>,
    },
    Call {
        name: Token<'src>,
        arguments: Vec<Self>,
    },
    Comparison {
        lhs: Box<Self>,
        operator: ConditionalOperator,
        rhs: Box<Self>,
        token: Token<'src>,
    },
    Concatenation {
        lhs: Box<Self>,
        operator: Token<'src>,
        rhs: Box<Self>,
    },
    Conditional {
        condition: Box<Self>,
        then: Box<Self>,
        otherwise: Option<Box<Self>>,
    },
    FormatString {
        start: StringLiteral<'src>,
        expressions: Vec<(Self, StringLiteral<'src>)>,
    },
    Group {
        contents: Box<Self>,
    },
    Join {
        lhs: Option<Box<Self>>,
        operator: Token<'src>,
        rhs: Box<Self>,
    },
    List {
        elements: Vec<Self>,
        open: Token<'src>,
    },
    ListConcatenation {
        lhs: Box<Self>,
        operator: Token<'src>,
        rhs: Box<Self>,
    },
    Not {
        operand: Box<Self>,
    },
    Or {
        lhs: Box<Self>,
        rhs: Box<Self>,
    },
    StringLiteral {
        string_literal: StringLiteral<'src>,
    },
    Variable {
        name: Token<'src>,
    },
}

impl Display for Expression<'_> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::And { lhs, rhs } => write!(f, "{lhs} && {rhs}"),
            Self::Assert {
                condition, message, ..
            } => match message {
                Some(message) => write!(f, "assert({condition}, {message})"),
                None => write!(f, "assert({condition})"),
            },
            Self::Backtick { token, .. } => write!(f, "{token}"),
            Self::Call { name, arguments } => {
                write!(f, "{name}(")?;
                write_separated(f, arguments)?;
                write!(f, ")")
            }
            Self::Comparison {
                lhs, operator, rhs, ..
            } => write!(f, "{lhs} {operator} {rhs}"),
            Self::Concatenation { lhs, rhs, .. } => write!(f, "{lhs} + {rhs}"),
            Self::ListConcatenation { lhs, rhs, .. } => write!(f, "{lhs} ++ {rhs}"),
            Self::Conditional {
                condition,
                then,
                otherwise,
            } => {
                write!(f, "if {condition} {{ {then} }}")?;
                match otherwise.as_deref() {
                    Some(otherwise @ Self::Conditional { .. }) => write!(f, " else {otherwise}"),
                    Some(otherwise) => write!(f, " else {{ {otherwise} }}"),
                    None => Ok(()),
                }
            }
            Self::FormatString { start, expressions } => {
                write!(f, "{start}")?;
                for (expression, string) in expressions {
                    write!(f, " {expression} {string}")?;
                }
                Ok(())
            }
            Self::Group { contents } => write!(f, "({contents})"),
            Self::Join { lhs: None, rhs, .. } => write!(f, "/ {rhs}"),
            Self::Join {
                lhs: Some(lhs),
                rhs,
                ..
            } => write!(f, "{lhs} / {rhs}"),
            Self::List { elements, .. } => {
                write!(f, "[")?;
                write_separated(f, elements)?;
                write!(f, "]")
            }
            Self::Not { operand } => write!(f, "!{operand}"),
            Self::Or { lhs, rhs } => write!(f, "{lhs} || {rhs}"),
            Self::StringLiteral { string_literal } => write!(f, "{string_literal}"),
            Self::Variable { name } => write!(f, "{name}"),
        }
    }
}

fn write_separated<T: Display>(f: &mut Formatter, items: &[T]) -> fmt::Result {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            write!(f, ", ")?;
        }
        write!(f, "{item}")?;
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Interpreter<T> {
    pub command: T,
    pub arguments: Vec<T>,
}

impl<T: Display> Display for Interpreter<T> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.command)?;
        for argument in &self.arguments {
            write!(f, ", {argument}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Signal {
    Hangup,
    Interrupt,
    Quit,
}

impl Display for Signal {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Self::Hangup => write!(f, "SIGHUP"),
            Self::Interrupt => write!(f, "SIGINT"),
            Self::Quit => write!(f, "SIGQUIT"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AttributeKind {
    Android,
    Arg,
    Cache,
    Confirm,
    Continue,
    Default,
    Doc,
    Dragonfly,
    Env,
    ExitMessage,
    Extension,
    Freebsd,
    Group,
    Linux,
    Macos,
    Metadata,
    Netbsd,
    NoCd,
    NoExitMessage,
    NoQuiet,
    Openbsd,
    Parallel,
    PositionalArguments,
    Private,
    Script,
    Shell,
    Timestamp,
    Unix,
    Windows,
    WorkingDirectory,
}

impl AttributeKind {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "android" => Some(Self::Android),
            "arg" => Some(Self::Arg),
            "cache" => Some(Self::Cache),
            "confirm" => Some(Self::Confirm),
            "continue" => Some(Self::Continue),
            "default" => Some(Self::Default),
            "doc" => Some(Self::Doc),
            "dragonfly" => Some(Self::Dragonfly),
            "env" => Some(Self::Env),
            "exit-message" => Some(Self::ExitMessage),
            "extension" => Some(Self::Extension),
            "freebsd" => Some(Self::Freebsd),
            "group" => Some(Self::Group),
            "linux" => Some(Self::Linux),
            "macos" => Some(Self::Macos),
            "metadata" => Some(Self::Metadata),
            "netbsd" => Some(Self::Netbsd),
            "no-cd" => Some(Self::NoCd),
            "no-exit-message" => Some(Self::NoExitMessage),
            "no-quiet" => Some(Self::NoQuiet),
            "openbsd" => Some(Self::Openbsd),
            "parallel" => Some(Self::Parallel),
            "positional-arguments" => Some(Self::PositionalArguments),
            "private" => Some(Self::Private),
            "script" => Some(Self::Script),
            "shell" => Some(Self::Shell),
            "timestamp" => Some(Self::Timestamp),
            "unix" => Some(Self::Unix),
            "windows" => Some(Self::Windows),
            "working-directory" => Some(Self::WorkingDirectory),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Android => "android",
            Self::Arg => "arg",
            Self::Cache => "cache",
            Self::Confirm => "confirm",
            Self::Continue => "continue",
            Self::Default => "default",
            Self::Doc => "doc",
            Self::Dragonfly => "dragonfly",
            Self::Env => "env",
            Self::ExitMessage => "exit-message",
            Self::Extension => "extension",
            Self::Freebsd => "freebsd",
            Self::Group => "group",
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Metadata => "metadata",
            Self::Netbsd => "netbsd",
            Self::NoCd => "no-cd",
            Self::NoExitMessage => "no-exit-message",
            Self::NoQuiet => "no-quiet",
            Self::Openbsd => "openbsd",
            Self::Parallel => "parallel",
            Self::PositionalArguments => "positional-arguments",
            Self::Private => "private",
            Self::Script => "script",
            Self::Shell => "shell",
            Self::Timestamp => "timestamp",
            Self::Unix => "unix",
            Self::Windows => "windows",
            Self::WorkingDirectory => "working-directory",
        }
    }

    pub fn accepts_expressions(self) -> bool {
        matches!(
            self,
            Self::Confirm | Self::Doc | Self::Env | Self::Timestamp | Self::WorkingDirectory
        )
    }

    pub fn accepts_keyword_arguments(self) -> bool {
        matches!(self, Self::Arg | Self::Cache)
    }

    pub fn is_enabler(self) -> bool {
        matches!(
            self,
            Self::Android
                | Self::Dragonfly
                | Self::Freebsd
                | Self::Linux
                | Self::Macos
                | Self::Netbsd
                | Self::Openbsd
                | Self::Unix
                | Self::Windows
        )
    }

    pub fn argument_range(self) -> (usize, usize) {
        match self {
            Self::Android
            | Self::Cache
            | Self::Default
            | Self::Dragonfly
            | Self::ExitMessage
            | Self::Freebsd
            | Self::Linux
            | Self::Macos
            | Self::Netbsd
            | Self::NoCd
            | Self::NoExitMessage
            | Self::NoQuiet
            | Self::Openbsd
            | Self::Parallel
            | Self::PositionalArguments
            | Self::Private
            | Self::Shell
            | Self::Unix
            | Self::Windows => (0, 0),
            Self::Confirm | Self::Doc | Self::Timestamp => (0, 1),
            Self::Continue | Self::Script => (0, usize::MAX),
            Self::Arg | Self::Extension | Self::Group | Self::WorkingDirectory => (1, 1),
            Self::Env => (2, 2),
            Self::Metadata => (1, usize::MAX),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ArgAttribute<'src> {
    pub flag: Option<Token<'src>>,
    pub help_property: Option<(Token<'src>, Expression<'src>)>,
    pub long: Option<StringLiteral<'src>>,
    pub long_key: Option<Token<'src>>,
    pub max: Option<u64>,
    pub max_key: Option<Token<'src>>,
    pub min: Option<u64>,
    pub min_key: Option<Token<'src>>,
    pub multiple: Option<Token<'src>>,
    pub name: StringLiteral<'src>,
    pub pattern_property: Option<(Token<'src>, Expression<'src>)>,
    pub short: Option<StringLiteral<'src>>,
    pub short_key: Option<Token<'src>>,
    pub value: Option<Expression<'src>>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Attribute<'src> {
    Android,
    Arg(Box<ArgAttribute<'src>>),
    Cache {
        extra: Option<Expression<'src>>,
        inputs: Option<Expression<'src>>,
        outputs: Option<Expression<'src>>,
    },
    Confirm(Option<Expression<'src>>),
    Continue(BTreeSet<Signal>),
    Default,
    Doc(Option<Expression<'src>>),
    Dragonfly,
    Env(Expression<'src>, Expression<'src>),
    ExitMessage,
    Extension(StringLiteral<'src>),
    Freebsd,
    Group(StringLiteral<'src>),
    Linux,
    Macos,
    Metadata(Vec<StringLiteral<'src>>),
    Netbsd,
    NoCd,
    NoExitMessage,
    NoQuiet,
    Openbsd,
    Parallel,
    PositionalArguments,
    Private,
    Script(Option<Interpreter<StringLiteral<'src>>>),
    Shell,
    Timestamp(Option<Expression<'src>>),
    Unix,
    Windows,
    WorkingDirectory(Expression<'src>),
}

impl Attribute<'_> {
    pub fn kind(&self) -> AttributeKind {
        match self {
            Self::Android => AttributeKind::Android,
            Self::Arg(_) => AttributeKind::Arg,
            Self::Cache { .. } => AttributeKind::Cache,
            Self::Confirm(_) => AttributeKind::Confirm,
            Self::Continue(_) => AttributeKind::Continue,
            Self::Default => AttributeKind::Default,
            Self::Doc(_) => AttributeKind::Doc,
            Self::Dragonfly => AttributeKind::Dragonfly,
            Self::Env(_, _) => AttributeKind::Env,
            Self::ExitMessage => AttributeKind::ExitMessage,
            Self::Extension(_) => AttributeKind::Extension,
            Self::Freebsd => AttributeKind::Freebsd,
            Self::Group(_) => AttributeKind::Group,
            Self::Linux => AttributeKind::Linux,
            Self::Macos => AttributeKind::Macos,
            Self::Metadata(_) => AttributeKind::Metadata,
            Self::Netbsd => AttributeKind::Netbsd,
            Self::NoCd => AttributeKind::NoCd,
            Self::NoExitMessage => AttributeKind::NoExitMessage,
            Self::NoQuiet => AttributeKind::NoQuiet,
            Self::Openbsd => AttributeKind::Openbsd,
            Self::Parallel => AttributeKind::Parallel,
            Self::PositionalArguments => AttributeKind::PositionalArguments,
            Self::Private => AttributeKind::Private,
            Self::Script(_) => AttributeKind::Script,
            Self::Shell => AttributeKind::Shell,
            Self::Timestamp(_) => AttributeKind::Timestamp,
            Self::Unix => AttributeKind::Unix,
            Self::Windows => AttributeKind::Windows,
            Self::WorkingDirectory(_) => AttributeKind::WorkingDirectory,
        }
    }

    pub fn repeatable(&self) -> bool {
        matches!(
            self,
            Self::Arg(_) | Self::Env(_, _) | Self::Group(_) | Self::Metadata(_)
        )
    }
}

impl Display for Attribute<'_> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}", self.kind().name())?;
        match self {
            Self::Arg(arg) => {
                write!(f, "({}", arg.name)?;
                match (&arg.long_key, &arg.long) {
                    (Some(_), _) => write!(f, ", long")?,
                    (None, Some(long)) => write!(f, ", long={long}")?,
                    (None, None) => {}
                }
                match (&arg.short_key, &arg.short) {
                    (Some(_), _) => write!(f, ", short")?,
                    (None, Some(short)) => write!(f, ", short={short}")?,
                    (None, None) => {}
                }
                if let Some((_key, pattern)) = &arg.pattern_property {
                    write!(f, ", pattern={pattern}")?;
                }
                if let Some(value) = &arg.value {
                    write!(f, ", value={value}")?;
                }
                if arg.flag.is_some() {
                    write!(f, ", flag")?;
                }
                if arg.multiple.is_some() {
                    write!(f, ", multiple")?;
                }
                if let Some(min) = arg.min {
                    write!(f, ", min='{min}'")?;
                }
                if let Some(max) = arg.max {
                    write!(f, ", max='{max}'")?;
                }
                if let Some((_key, help)) = &arg.help_property {
                    write!(f, ", help={help}")?;
                }
                write!(f, ")")
            }
            Self::Android
            | Self::Confirm(None)
            | Self::Default
            | Self::Doc(None)
            | Self::Dragonfly
            | Self::ExitMessage
            | Self::Freebsd
            | Self::Linux
            | Self::Macos
            | Self::Netbsd
            | Self::NoCd
            | Self::NoExitMessage
            | Self::NoQuiet
            | Self::Openbsd
            | Self::Parallel
            | Self::PositionalArguments
            | Self::Private
            | Self::Script(None)
            | Self::Shell
            | Self::Timestamp(None)
            | Self::Unix
            | Self::Windows => Ok(()),
            Self::Cache {
                extra,
                inputs,
                outputs,
            } => {
                let mut arguments = Vec::new();
                if let Some(extra) = extra {
                    arguments.push(format!("extra={extra}"));
                }
                if let Some(inputs) = inputs {
                    arguments.push(format!("inputs={inputs}"));
                }
                if let Some(outputs) = outputs {
                    arguments.push(format!("outputs={outputs}"));
                }
                if arguments.is_empty() {
                    Ok(())
                } else {
                    write!(f, "({})", arguments.join(", "))
                }
            }
            Self::Confirm(Some(argument))
            | Self::Doc(Some(argument))
            | Self::Timestamp(Some(argument))
            | Self::WorkingDirectory(argument) => write!(f, "({argument})"),
            Self::Extension(argument) | Self::Group(argument) => write!(f, "({argument})"),
            Self::Continue(signals) => {
                if signals.is_empty() {
                    return Ok(());
                }
                write!(f, "(")?;
                for (i, signal) in signals.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "\"{signal}\"")?;
                }
                write!(f, ")")
            }
            Self::Env(key, value) => write!(f, "({key}, {value})"),
            Self::Metadata(arguments) => {
                write!(f, "(")?;
                write_separated(f, arguments)?;
                write!(f, ")")
            }
            Self::Script(Some(shell)) => write!(f, "({shell})"),
        }
    }
}

pub type AttributeSet<'src> = BTreeSet<Attribute<'src>>;

pub fn contains(attributes: &AttributeSet, kind: AttributeKind) -> bool {
    attributes.iter().any(|attribute| attribute.kind() == kind)
}

fn is_enabled(attributes: &AttributeSet) -> bool {
    let android = contains(attributes, AttributeKind::Android);
    let dragonfly = contains(attributes, AttributeKind::Dragonfly);
    let freebsd = contains(attributes, AttributeKind::Freebsd);
    let linux = contains(attributes, AttributeKind::Linux);
    let macos = contains(attributes, AttributeKind::Macos);
    let netbsd = contains(attributes, AttributeKind::Netbsd);
    let openbsd = contains(attributes, AttributeKind::Openbsd);
    let unix = contains(attributes, AttributeKind::Unix);
    let windows = contains(attributes, AttributeKind::Windows);

    (!windows
        && !linux
        && !macos
        && !openbsd
        && !freebsd
        && !dragonfly
        && !netbsd
        && !unix
        && !android)
        || (cfg!(target_os = "android") && android)
        || (cfg!(target_os = "dragonfly") && dragonfly)
        || (cfg!(target_os = "freebsd") && freebsd)
        || (cfg!(target_os = "linux") && linux)
        || (cfg!(target_os = "macos") && macos)
        || (cfg!(target_os = "netbsd") && netbsd)
        || (cfg!(target_os = "openbsd") && openbsd)
        || (cfg!(unix) && unix)
        || (cfg!(windows) && windows)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterKind {
    Plus,
    Singular,
    Star,
}

#[derive(Debug, Clone)]
pub struct Parameter<'src> {
    pub default: Option<Expression<'src>>,
    pub export: bool,
    pub kind: ParameterKind,
    pub name: Token<'src>,
}

impl Display for Parameter<'_> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self.kind {
            ParameterKind::Plus => write!(f, "+")?,
            ParameterKind::Star => write!(f, "*")?,
            ParameterKind::Singular => {}
        }
        if self.export {
            write!(f, "$")?;
        }
        write!(f, "{}", self.name)?;
        if let Some(default) = &self.default {
            write!(f, "={default}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct DependencyArgument<'src> {
    pub expression: Expression<'src>,
    pub starred: bool,
}

#[derive(Debug, Clone)]
pub struct Dependency<'src> {
    pub arguments: Vec<DependencyArgument<'src>>,
    pub recipe: Vec<Token<'src>>,
}

impl Display for Dependency<'_> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        if self.arguments.is_empty() {
            return write_namepath(f, &self.recipe);
        }
        if self.arguments.iter().any(|argument| argument.starred) {
            write!(f, "*")?;
        }
        write!(f, "(")?;
        write_namepath(f, &self.recipe)?;
        for argument in &self.arguments {
            write!(f, " ")?;
            if argument.starred {
                write!(f, "*")?;
            }
            write!(f, "{}", argument.expression)?;
        }
        write!(f, ")")
    }
}

fn write_namepath(f: &mut Formatter, path: &[Token]) -> fmt::Result {
    for (i, name) in path.iter().enumerate() {
        if i > 0 {
            write!(f, "::")?;
        }
        write!(f, "{name}")?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub enum Fragment<'src> {
    Interpolation(Expression<'src>),
    Text(Token<'src>),
}

#[derive(Debug, Clone)]
pub struct Recipe<'src> {
    pub attributes: AttributeSet<'src>,
    pub body: Vec<Vec<Fragment<'src>>>,
    pub dependencies: Vec<Dependency<'src>>,
    pub doc: Option<String>,
    pub name: Token<'src>,
    pub parameters: Vec<Parameter<'src>>,
    pub priors: usize,
    pub quiet: bool,
}

#[derive(Debug, Clone)]
pub enum Setting<'src> {
    Bool(bool),
    Expression(Expression<'src>),
    Indentation(Expression<'src>, String),
    Interpreter(Interpreter<Expression<'src>>),
}

#[derive(Debug, Clone)]
pub enum Item<'src> {
    Alias {
        attributes: AttributeSet<'src>,
        name: Token<'src>,
        target: Vec<Token<'src>>,
    },
    Assignment {
        attributes: AttributeSet<'src>,
        eager: bool,
        export: bool,
        name: Token<'src>,
        value: Expression<'src>,
    },
    Comment(&'src str),
    Function {
        attributes: AttributeSet<'src>,
        name: Token<'src>,
        parameters: Vec<Token<'src>>,
        body: Expression<'src>,
    },
    Import {
        attributes: AttributeSet<'src>,
        optional: bool,
        relative: StringLiteral<'src>,
    },
    Module {
        attributes: AttributeSet<'src>,
        doc: Option<String>,
        name: Token<'src>,
        optional: bool,
        relative: Option<StringLiteral<'src>>,
    },
    Newline,
    Recipe(Box<Recipe<'src>>),
    Setting {
        attributes: AttributeSet<'src>,
        name: Token<'src>,
        value: Setting<'src>,
    },
    Unexport {
        attributes: AttributeSet<'src>,
        name: Token<'src>,
    },
}

impl<'src> Item<'src> {
    fn attributes(&self) -> Option<&AttributeSet<'src>> {
        match self {
            Self::Alias { attributes, .. }
            | Self::Assignment { attributes, .. }
            | Self::Function { attributes, .. }
            | Self::Import { attributes, .. }
            | Self::Module { attributes, .. }
            | Self::Setting { attributes, .. }
            | Self::Unexport { attributes, .. } => Some(attributes),
            Self::Recipe(recipe) => Some(&recipe.attributes),
            Self::Comment(_) | Self::Newline => None,
        }
    }

    fn doc_comment(&self) -> Option<&str> {
        match self {
            Self::Module {
                attributes, doc, ..
            } => {
                if contains(attributes, AttributeKind::Doc) {
                    None
                } else {
                    doc.as_deref()
                }
            }
            Self::Recipe(recipe) => {
                if contains(&recipe.attributes, AttributeKind::Doc) {
                    None
                } else {
                    recipe.doc.as_deref()
                }
            }
            _ => None,
        }
    }

    fn write(&self, out: &mut String, indentation: &str) -> fmt::Result {
        if let Some(doc) = self.doc_comment() {
            if doc.is_empty() {
                writeln!(out, "#")?;
            } else {
                writeln!(out, "# {doc}")?;
            }
        }

        if let Some(attributes) = self.attributes() {
            for attribute in attributes {
                writeln!(out, "[{attribute}]")?;
            }
        }

        match self {
            Self::Alias { name, target, .. } => {
                write!(out, "alias {name} := ")?;
                for (i, name) in target.iter().enumerate() {
                    if i > 0 {
                        write!(out, "::")?;
                    }
                    write!(out, "{name}")?;
                }
                Ok(())
            }
            Self::Assignment {
                eager,
                export,
                name,
                value,
                ..
            } => {
                if *eager {
                    write!(out, "eager ")?;
                }
                if *export {
                    write!(out, "export ")?;
                }
                write!(out, "{name} := {value}")
            }
            Self::Comment(comment) => write!(out, "{comment}"),
            Self::Function {
                name,
                parameters,
                body,
                ..
            } => {
                write!(out, "{name}(")?;
                for (i, parameter) in parameters.iter().enumerate() {
                    if i > 0 {
                        write!(out, ", ")?;
                    }
                    write!(out, "{parameter}")?;
                }
                write!(out, ") := {body}")
            }
            Self::Import {
                optional, relative, ..
            } => {
                write!(out, "import")?;
                if *optional {
                    write!(out, "?")?;
                }
                write!(out, " {relative}")
            }
            Self::Module {
                name,
                optional,
                relative,
                ..
            } => {
                write!(out, "mod")?;
                if *optional {
                    write!(out, "?")?;
                }
                write!(out, " {name}")?;
                if let Some(path) = relative {
                    write!(out, " {path}")?;
                }
                Ok(())
            }
            Self::Newline => Ok(()),
            Self::Recipe(recipe) => write_recipe(out, recipe, indentation),
            Self::Setting { name, value, .. } => match value {
                Setting::Bool(true) => write!(out, "set {name}"),
                Setting::Bool(false) => write!(out, "set {name} := false"),
                Setting::Expression(value) | Setting::Indentation(value, _) => {
                    write!(out, "set {name} := {value}")
                }
                Setting::Interpreter(value) => write!(out, "set {name} := [{value}]"),
            },
            Self::Unexport { name, .. } => write!(out, "unexport {name}"),
        }
    }
}

fn write_recipe(out: &mut String, recipe: &Recipe, indentation: &str) -> fmt::Result {
    if recipe.quiet {
        write!(out, "@")?;
    }
    write!(out, "{}", recipe.name)?;
    for parameter in &recipe.parameters {
        write!(out, " {parameter}")?;
    }
    write!(out, ":")?;
    for (i, dependency) in recipe.dependencies.iter().enumerate() {
        if i == recipe.priors {
            write!(out, " &&")?;
        }
        write!(out, " {dependency}")?;
    }
    for (i, line) in recipe.body.iter().enumerate() {
        if i == 0 {
            writeln!(out)?;
        }
        for (j, fragment) in line.iter().enumerate() {
            if j == 0 {
                write!(out, "{indentation}")?;
            }
            match fragment {
                Fragment::Text(token) => write!(out, "{token}")?,
                Fragment::Interpolation(expression) => write!(out, "{{{{ {expression} }}}}")?,
            }
        }
        if i + 1 < recipe.body.len() {
            writeln!(out)?;
        }
    }
    Ok(())
}

fn indentation(items: &[Item]) -> String {
    for item in items {
        if let Item::Setting {
            attributes,
            value: Setting::Indentation(_, indentation),
            ..
        } = item
            && is_enabled(attributes)
        {
            return indentation.clone();
        }
    }
    "    ".to_string()
}

pub fn format(items: &[Item]) -> Result<String, fmt::Error> {
    let indentation = indentation(items);
    let mut out = String::new();
    let mut newlines = 0;
    for (i, item) in items.iter().enumerate() {
        if matches!(item, Item::Newline) {
            newlines += 1;
            continue;
        }

        match newlines {
            0 => {}
            1 => out.push('\n'),
            _ => out.push_str("\n\n"),
        }
        newlines = 0;

        if i > 0 && !matches!(items[i - 1], Item::Newline) {
            out.push(' ');
        }

        item.write(&mut out, &indentation)?;
    }

    if !items.is_empty() {
        out.push('\n');
    }

    Ok(out)
}
