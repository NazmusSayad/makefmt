use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use libtest_mimic::{Arguments, Failed, Trial};

const JUST_VERSION: &str = "just 1.58.0";
const BLESS_VARIABLE: &str = "JUSTFILE_FIXTURES_BLESS";
const GENERATED_CASES: u64 = 6000;
const MUTATION_CASES: u64 = 4000;
const FORMAT_STACK_SIZE: usize = 64 * 1024 * 1024;
const BLANK_LINES: usize = 200_000;
const BLANK_LINES_TIME_LIMIT: Duration = Duration::from_secs(10);
const DEEPEST_NESTING: usize = 1021;

fn main() {
    let arguments = Arguments::from_args();
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/justfile");
    let just = Arc::new(check_just());
    let bless = std::env::var_os(BLESS_VARIABLE).is_some();

    let fixtures = fixture_paths(&directory);
    let inputs = Arc::new(
        fixtures
            .iter()
            .map(|path| read(path))
            .collect::<Vec<String>>(),
    );

    let mut trials = Vec::new();

    for path in fixtures {
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("fixture names are UTF-8")
            .to_string();
        let just = just.clone();
        trials.push(Trial::test(format!("fixture::{name}"), move || {
            if bless {
                bless_fixture(&path, &just)
            } else {
                check_fixture(&path)
            }
        }));
    }

    trials.push(Trial::test("performance::blank_lines", check_blank_lines));
    trials.push(Trial::test("limits::nesting", check_nesting_limit));

    for seed in 0..GENERATED_CASES {
        let just = just.clone();
        trials.push(Trial::test(format!("generated::{seed:05}"), move || {
            check_generated(seed, &just)
        }));
    }

    for seed in 0..MUTATION_CASES {
        let just = just.clone();
        let inputs = inputs.clone();
        trials.push(Trial::test(format!("mutation::{seed:05}"), move || {
            check_mutation(seed, &inputs, &just)
        }));
    }

    libtest_mimic::run(&arguments, trials).exit();
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("failed to read {path:?}: {error}"))
}

fn fixture_paths(directory: &Path) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {directory:?}: {error}"))
        .map(|entry| entry.expect("failed to read fixture entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "just")
        })
        .collect::<Vec<PathBuf>>();
    paths.sort();
    paths
}

fn format(source: &str) -> Result<String, String> {
    let source = source.to_string();
    thread::Builder::new()
        .stack_size(FORMAT_STACK_SIZE)
        .spawn(move || makefmt::format_justfile(&source))
        .expect("failed to spawn formatter thread")
        .join()
        .expect("format_justfile panicked")
}

fn check_just() -> Result<(), String> {
    let output = Command::new("just").arg("--version").output().map_err(|error| {
        format!("differential tests require `{JUST_VERSION}` on PATH, but running `just` failed: {error}")
    })?;
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version == JUST_VERSION {
        Ok(())
    } else {
        Err(format!(
            "differential tests require `{JUST_VERSION}` on PATH, but found `{version}`"
        ))
    }
}

fn run_just(just: &Result<(), String>, arguments: &[&str], source: &str) -> Result<Output, Failed> {
    just.clone()?;
    let mut command = Command::new("just");
    command
        .args(arguments)
        .args(["--justfile", "-"])
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, _value) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("JUST_") {
            command.env_remove(key);
        }
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("failed to run just: {error}"))?;
    let mut stdin = child.stdin.take().ok_or("failed to open just stdin")?;
    let input = source.as_bytes().to_vec();
    let writer = thread::spawn(move || stdin.write_all(&input));
    let output = child
        .wait_with_output()
        .map_err(|error| format!("failed to wait for just: {error}"))?;
    writer
        .join()
        .map_err(|_| "just stdin writer panicked")?
        .map_err(|error| format!("failed to write just stdin: {error}"))?;
    Ok(output)
}

fn just_format(just: &Result<(), String>, source: &str) -> Result<Result<String, String>, Failed> {
    let output = run_just(just, &["--fmt"], source)?;
    let stdout = String::from_utf8(output.stdout).map_err(|_| "just printed invalid UTF-8")?;
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    match output.status.code() {
        Some(0) => Ok(Ok(stdout)),
        Some(1) => Ok(Err(stderr)),
        code => Err(format!("just --fmt exited with {code:?}:\n{stderr}").into()),
    }
}

fn just_dump(just: &Result<(), String>, source: &str) -> Result<String, Failed> {
    let output = run_just(just, &["--dump", "--dump-format", "json"], source)?;
    if !output.status.success() {
        return Err(format!(
            "just --dump failed:\n{}\nsource:\n{source}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let dump = String::from_utf8(output.stdout).map_err(|_| "just printed invalid UTF-8")?;
    let mut normalized = String::new();
    let mut rest = dump.as_str();
    while let Some(start) = rest.find("\"source\":\"") {
        let value = start + "\"source\":\"".len();
        let end = rest[value..]
            .find('"')
            .ok_or("unterminated source path in dump")?;
        normalized.push_str(&rest[..value]);
        rest = &rest[value + end..];
    }
    normalized.push_str(rest);
    Ok(normalized)
}

fn mismatch(source: &str, expected: &str, actual: &str) -> Failed {
    format!(
        "output differs from `just --fmt`\n--- source:\n{source}\n--- just --fmt:\n{expected}\n--- format_justfile:\n{actual}\n--- escaped:\n{expected:?}\n{actual:?}"
    )
    .into()
}

fn check_idempotent(formatted: &str) -> Result<(), Failed> {
    let again = format(formatted)
        .map_err(|error| format!("formatting the output failed: {error}\n{formatted}"))?;
    if again != formatted {
        return Err(format!(
            "formatting is not idempotent\n--- first:\n{formatted}\n--- second:\n{again}"
        )
        .into());
    }
    Ok(())
}

fn check_fixture(path: &Path) -> Result<(), Failed> {
    let source = read(path);
    let expected_path = path.with_extension("expected");
    let error_path = path.with_extension("error");
    match (expected_path.exists(), error_path.exists()) {
        (true, false) => {
            let expected = read(&expected_path);
            let actual = format(&source).map_err(|error| {
                format!("format_justfile failed but `just --fmt` succeeds: {error}")
            })?;
            if actual != expected {
                return Err(mismatch(&source, &expected, &actual));
            }
            check_idempotent(&actual)
        }
        (false, true) => match format(&source) {
            Ok(output) => Err(format!(
                "format_justfile succeeded but `just --fmt` fails with:\n{}\n--- output:\n{output}",
                read(&error_path)
            )
            .into()),
            Err(_) => Ok(()),
        },
        (true, true) => Err(format!("{path:?} has both .expected and .error files").into()),
        (false, false) => Err(format!(
            "{path:?} has no .expected or .error file, run with {BLESS_VARIABLE}=1 to generate them"
        )
        .into()),
    }
}

fn bless_fixture(path: &Path, just: &Result<(), String>) -> Result<(), Failed> {
    let source = read(path);
    let expected_path = path.with_extension("expected");
    let error_path = path.with_extension("error");
    let (write_path, content, remove_path) = match just_format(just, &source)? {
        Ok(formatted) => (expected_path, formatted, error_path),
        Err(stderr) => (error_path, normalize_error(&stderr), expected_path),
    };
    fs::write(&write_path, content)
        .map_err(|error| format!("failed to write {write_path:?}: {error}"))?;
    if remove_path.exists() {
        fs::remove_file(&remove_path)
            .map_err(|error| format!("failed to remove {remove_path:?}: {error}"))?;
    }
    Ok(())
}

fn normalize_error(stderr: &str) -> String {
    let mut normalized = String::new();
    for line in stderr.lines() {
        match line.split_once("——▶ ") {
            Some((prefix, location)) => match location.split_once("/justfile:") {
                Some((_, position)) => {
                    normalized.push_str(&format!("{prefix}——▶ justfile:{position}"))
                }
                None => normalized.push_str(line),
            },
            None => normalized.push_str(line),
        }
        normalized.push('\n');
    }
    normalized
}

fn check_blank_lines() -> Result<(), Failed> {
    let blank = "\n".repeat(BLANK_LINES);
    let indented_blank = "  \n".repeat(BLANK_LINES);
    let cases = [
        ("blank file", blank.clone(), "\n".to_string()),
        (
            "blank lines between recipes",
            format!("a:\n  echo\n{blank}b:\n  echo\n"),
            "a:\n    echo\n\nb:\n    echo\n".to_string(),
        ),
        (
            "blank lines in recipe body",
            format!("a:\n  echo\n{indented_blank}  echo\n"),
            format!("a:\n    echo\n{blank}    echo\n"),
        ),
    ];
    for (name, source, expected) in cases {
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .stack_size(FORMAT_STACK_SIZE)
            .spawn(move || sender.send(makefmt::format_justfile(&source)))
            .map_err(|error| format!("failed to spawn formatter thread: {error}"))?;
        let actual = match receiver.recv_timeout(BLANK_LINES_TIME_LIMIT) {
            Ok(result) => result.map_err(|error| format!("{name}: {error}"))?,
            Err(RecvTimeoutError::Timeout) => {
                return Err(format!(
                    "{name}: formatting took longer than {BLANK_LINES_TIME_LIMIT:?}"
                )
                .into());
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(format!("{name}: format_justfile panicked").into());
            }
        };
        if actual != expected {
            return Err(format!("{name}: output differs from `just --fmt`").into());
        }
    }
    Ok(())
}

fn check_nesting_limit() -> Result<(), Failed> {
    let nested = |depth: usize| format!("a := {}'x'\n", "!".repeat(depth));
    let deepest = nested(DEEPEST_NESTING);
    let formatted = format(&deepest)
        .map_err(|error| format!("nesting depth {DEEPEST_NESTING} was rejected: {error}"))?;
    if formatted != deepest {
        return Err(format!("nesting depth {DEEPEST_NESTING} was not preserved").into());
    }
    for depth in [DEEPEST_NESTING + 1, 1_000_000] {
        match format(&nested(depth)) {
            Ok(_) => return Err(format!("nesting depth {depth} was accepted").into()),
            Err(error) if error.ends_with("expression nesting too deep") => {}
            Err(error) => return Err(format!("nesting depth {depth} failed with: {error}").into()),
        }
    }
    Ok(())
}

fn check_generated(seed: u64, just: &Result<(), String>) -> Result<(), Failed> {
    let source = Generator::new(seed).justfile();
    let expected = just_format(just, &source)?.map_err(|stderr| {
        format!("just rejected generated justfile:\n{stderr}\n--- source:\n{source}")
    })?;
    let actual = format(&source)
        .map_err(|error| format!("format_justfile failed: {error}\n--- source:\n{source}"))?;
    if actual != expected {
        return Err(mismatch(&source, &expected, &actual));
    }
    check_idempotent(&actual)?;
    let before = just_dump(just, &source)?;
    let after = just_dump(just, &actual)?;
    if before != after {
        return Err(format!("dump changed after formatting\n--- before:\n{before}\n--- after:\n{after}\n--- source:\n{source}").into());
    }
    Ok(())
}

fn check_mutation(seed: u64, inputs: &[String], just: &Result<(), String>) -> Result<(), Failed> {
    let mut rng = Rng::new(seed ^ 0x6d75_7461_7465);
    let source = if rng.chance(50) || inputs.is_empty() {
        Generator::new(GENERATED_CASES + seed).justfile()
    } else {
        rng.pick(inputs).clone()
    };
    let source = mutate(&mut rng, &source);
    match just_format(just, &source)? {
        Ok(expected) => {
            let actual = format(&source).map_err(|error| {
                format!("format_justfile failed but `just --fmt` succeeds: {error}\n--- source:\n{source:?}")
            })?;
            if actual != expected {
                return Err(mismatch(&source, &expected, &actual));
            }
            check_idempotent(&actual)
        }
        Err(stderr) if stderr.contains("shell expansion failed") => Ok(()),
        Err(stderr) => match format(&source) {
            Ok(output) => Err(format!(
                "format_justfile succeeded but `just --fmt` fails with:\n{stderr}\n--- source:\n{source:?}\n--- output:\n{output}"
            )
            .into()),
            Err(_) => Ok(()),
        },
    }
}

const MUTATION_PIECES: &[&str] = &[
    "{{",
    "}}",
    "\"",
    "'",
    "`",
    "\\",
    "\n",
    "\t",
    " ",
    "  ",
    "#",
    "[",
    "]",
    "(",
    ")",
    ":",
    ":=",
    "=",
    "@",
    "$",
    "+",
    "*",
    "&&",
    "||",
    "!",
    "f",
    "x",
    "if",
    "else",
    "\r\n",
    "\r",
    "\u{feff}",
    "é",
    "/",
    ",",
    "::",
    "\"\"\"",
    "'''",
    "```",
    "\n    ",
    "\n\t",
    "f\"",
    "f'",
    "{{{{",
    "#!",
    "==",
    "=~",
    "!=",
    "++",
    "?",
    "\\\n",
    "a",
    "-",
    "{",
    "}",
    "set ",
    "alias ",
    "mod? ",
    "[private]",
    "import? ",
    "export ",
    "\n\n",
];

fn mutate(rng: &mut Rng, source: &str) -> String {
    let mut chars = source.chars().collect::<Vec<char>>();
    for _ in 0..1 + rng.below(3) {
        let index = rng.below(chars.len() + 1);
        match rng.below(4) {
            0 if !chars.is_empty() => {
                let end = (index + 1 + rng.below(4)).min(chars.len());
                chars.drain(index.min(chars.len())..end);
            }
            1 => {
                let piece = rng.pick(MUTATION_PIECES).chars().collect::<Vec<char>>();
                chars.splice(index..index, piece);
            }
            2 if !chars.is_empty() => {
                let end = (index + 1 + rng.below(10)).min(chars.len());
                let span = chars[index.min(end)..end].to_vec();
                let target = rng.below(chars.len() + 1);
                chars.splice(target..target, span);
            }
            _ => {
                let piece = rng.pick(MUTATION_PIECES).chars().collect::<Vec<char>>();
                let end = (index + 1).min(chars.len());
                chars.splice(index..end, piece);
            }
        }
    }
    chars.into_iter().collect()
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x2545_f491_4f6c_dd1d)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Type {
    String,
    List,
}

#[derive(Clone, Copy, PartialEq)]
enum Variadic {
    None,
    Plus,
    Star,
}

struct Signature {
    required: usize,
    optional: usize,
    variadic: Variadic,
    referable: bool,
}

const FUNCTIONS: &[(&str, usize)] = &[
    ("uppercase", 1),
    ("lowercase", 1),
    ("trim", 1),
    ("quote", 1),
    ("replace", 3),
    ("join", 2),
    ("join", 3),
    ("kebabcase", 1),
    ("capitalize", 1),
    ("trim_end_match", 2),
    ("append", 2),
    ("prepend", 2),
    ("sha256", 1),
    ("encode_uri_component", 1),
    ("arch", 0),
    ("os", 0),
    ("os_family", 0),
    ("num_cpus", 0),
    ("env", 2),
    ("env_var_or_default", 2),
];

const WORDS: &[&str] = &[
    "a",
    "b",
    "hello",
    "world",
    "foo bar",
    "x-y",
    "ü",
    "日本",
    "🎉",
    "",
    "with space",
    "1.2.3",
    "path/to",
    "{}",
    "tab\tin",
];

const REGEXES: &[&str] = &["^a", "b$", ".*", "[0-9]+", "x|y", "^$"];

const BODY_TEXT: &[&str] = &[
    "echo hello",
    "echo \"quoted $HOME\"",
    "ls -la",
    "cargo build --release",
    "printf '%s\\n' a b",
    "true",
    "echo 'single'",
    "echo ünïcödé ✓",
    "cd dir && make",
    "echo $((1 + 2))",
];

const INDENTS: &[&str] = &[" ", "  ", "    ", "\t", "        "];

const SHEBANGS: &[&str] = &["#!/usr/bin/env bash", "#!/bin/sh", "#!/usr/bin/env python3"];

struct Generator {
    rng: Rng,
    lists: bool,
    variables: Vec<Type>,
    functions: Vec<usize>,
    recipes: Vec<Signature>,
    default_used: bool,
}

impl Generator {
    fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
            lists: false,
            variables: Vec::new(),
            functions: Vec::new(),
            recipes: Vec::new(),
            default_used: false,
        }
    }

    fn ws(&mut self) -> &'static str {
        self.rng.pick(&[" ", " ", " ", " ", "  ", "\t", "   "])
    }

    fn ows(&mut self) -> &'static str {
        self.rng.pick(&["", "", " ", " ", " ", "  ", "\t"])
    }

    fn word(&mut self) -> &'static str {
        self.rng.pick(WORDS)
    }

    fn string_literal(&mut self) -> String {
        let word = self.word();
        match self.rng.below(14) {
            0..=4 => format!("\"{}\"", word.replace('\t', "\\t")),
            5..=8 => format!("'{word}'"),
            9 => format!(
                "\"{}\"",
                self.rng.pick(&[
                    "\\n",
                    "\\t",
                    "\\\\",
                    "\\\"",
                    "\\u{1F600}",
                    "\\u{41}",
                    "a\\nb"
                ])
            ),
            10 => {
                let indent = self.rng.pick(INDENTS);
                format!("\"\"\"\n{indent}{word}\n{indent}  more\n{indent}\"\"\"")
            }
            11 => {
                let indent = self.rng.pick(INDENTS);
                format!("'''\n{indent}{word}\n{indent}'''")
            }
            12 => format!("x'~/{word}'"),
            _ => {
                if self.rng.chance(50) {
                    format!("'''{word}'''")
                } else {
                    format!("\"\"\"{word}\"\"\"")
                }
            }
        }
    }

    fn regex(&mut self) -> String {
        format!("'{}'", self.rng.pick(REGEXES))
    }

    fn comparison_text(&mut self, scope: &[String], depth: usize) -> String {
        let operator = *self.rng.pick(&["==", "!=", "=~", "!~"]);
        let lhs = self.conjunct(scope, depth + 1);
        let rhs = if operator.contains('~') {
            self.regex()
        } else {
            self.conjunct(scope, depth + 1)
        };
        let ws1 = self.ows();
        let ws2 = self.ows();
        format!("{lhs}{ws1}{operator}{ws2}{rhs}")
    }

    fn condition(&mut self, scope: &[String], depth: usize) -> String {
        if self.lists && self.rng.chance(30) {
            self.string_expression(scope, depth + 1)
        } else {
            self.comparison_text(scope, depth)
        }
    }

    fn conditional(&mut self, scope: &[String], depth: usize, kind: Type) -> String {
        let condition = self.condition(scope, depth);
        let then = self.expression(scope, depth + 1, kind);
        let ws1 = self.ws();
        let ws2 = self.ows();
        let ws3 = self.ows();
        let ws4 = self.ows();
        let mut text = format!("if{ws1}{condition}{ws2}{{{ws3}{then}{ws4}}}");
        if self.lists && kind == Type::String && self.rng.chance(15) {
            return text;
        }
        loop {
            let ws = self.ows();
            text.push_str(ws);
            text.push_str("else");
            if self.rng.chance(30) {
                let ws = self.ws();
                let condition = self.condition(scope, depth);
                let then = self.expression(scope, depth + 1, kind);
                let ws2 = self.ows();
                let ws3 = self.ows();
                let ws4 = self.ows();
                text.push_str(&format!("{ws}if {condition}{ws2}{{{ws3}{then}{ws4}}}"));
                continue;
            }
            let otherwise = self.expression(scope, depth + 1, kind);
            let ws1 = self.ows();
            let ws2 = self.ows();
            let ws3 = self.ows();
            text.push_str(&format!("{ws1}{{{ws2}{otherwise}{ws3}}}"));
            return text;
        }
    }

    fn call(&mut self, scope: &[String], depth: usize) -> String {
        if !self.functions.is_empty() && self.rng.chance(25) {
            let index = self.rng.below(self.functions.len());
            let arity = self.functions[index];
            let arguments = (0..arity)
                .map(|_| self.string_expression(scope, depth + 1))
                .collect::<Vec<String>>();
            return self.call_text(&format!("fun{index}"), &arguments);
        }
        let (name, arity) = *self.rng.pick(FUNCTIONS);
        let arguments = (0..arity)
            .map(|_| self.string_expression(scope, depth + 1))
            .collect::<Vec<String>>();
        self.call_text(name, &arguments)
    }

    fn call_text(&mut self, name: &str, arguments: &[String]) -> String {
        let mut text = format!("{name}{}(", self.ows());
        for (i, argument) in arguments.iter().enumerate() {
            if i > 0 {
                text.push(',');
                text.push_str(self.ows());
            }
            text.push_str(self.ows());
            text.push_str(argument);
            text.push_str(self.ows());
        }
        if !arguments.is_empty() && self.rng.chance(10) {
            text.push(',');
        }
        text.push(')');
        text
    }

    fn string_value(&mut self, scope: &[String], depth: usize) -> String {
        let strings = scope
            .iter()
            .filter(|name| !name.starts_with('#'))
            .cloned()
            .collect::<Vec<String>>();
        let choice = if depth > 3 {
            self.rng.below(3)
        } else {
            self.rng.below(10)
        };
        match choice {
            0 | 1 => self.string_literal(),
            2 if !strings.is_empty() => self.rng.pick(&strings).clone(),
            2 => self.string_literal(),
            3 => self.call(scope, depth),
            4 => {
                let inner = self.string_expression(scope, depth + 1);
                let ws1 = self.ows();
                let ws2 = self.ows();
                format!("({ws1}{inner}{ws2})")
            }
            5 => match self.rng.below(3) {
                0 => "`echo hi`".to_string(),
                1 => "```\n    echo indented\n  ```".to_string(),
                _ => "``".to_string(),
            },
            6 => {
                let inner = self.string_expression(scope, depth + 1);
                let ws1 = self.ows();
                let ws2 = self.ows();
                let quote = *self.rng.pick(&["'", "\""]);
                let word = self.word().replace('\t', " ");
                format!("f{quote}{word} {{{{{ws1}{inner}{ws2}}}}} end{quote}")
            }
            7 if self.lists => {
                let inner = self.string_value(scope, depth + 1);
                format!("!{}{inner}", self.ows())
            }
            8 if self.lists => {
                let literal = self.string_literal();
                let message = self.string_literal();
                format!("assert({literal} == {literal}, {message})")
            }
            _ => self.string_literal(),
        }
    }

    fn conjunct(&mut self, scope: &[String], depth: usize) -> String {
        let choice = if depth > 4 { 0 } else { self.rng.below(8) };
        match choice {
            0..=2 => self.string_value(scope, depth),
            3 => self.conditional(scope, depth, Type::String),
            4 => {
                let rhs = self.conjunct(scope, depth + 1);
                format!("/{}{rhs}", self.ows())
            }
            _ => {
                let lhs = self.string_value(scope, depth + 1);
                let rhs = self.conjunct(scope, depth + 1);
                let operator = *self.rng.pick(&["+", "/", "+"]);
                let ws1 = self.ows();
                let ws2 = self.ows();
                format!("{lhs}{ws1}{operator}{ws2}{rhs}")
            }
        }
    }

    fn comparison(&mut self, scope: &[String], depth: usize) -> String {
        if self.lists && depth <= 4 && self.rng.chance(10) {
            self.comparison_text(scope, depth)
        } else {
            self.conjunct(scope, depth)
        }
    }

    fn disjunct(&mut self, scope: &[String], depth: usize) -> String {
        if self.lists && depth <= 4 && self.rng.chance(15) {
            let lhs = self.comparison(scope, depth + 1);
            let rhs = self.disjunct(scope, depth + 1);
            format!("{lhs}{}&&{}{rhs}", self.ows(), self.ows())
        } else {
            self.comparison(scope, depth)
        }
    }

    fn string_expression(&mut self, scope: &[String], depth: usize) -> String {
        if self.lists && depth <= 4 && self.rng.chance(15) {
            let lhs = self.disjunct(scope, depth + 1);
            let rhs = self.string_expression(scope, depth + 1);
            format!("{lhs}{}||{}{rhs}", self.ows(), self.ows())
        } else {
            self.disjunct(scope, depth)
        }
    }

    fn list_value(&mut self, scope: &[String], depth: usize) -> String {
        let lists = scope
            .iter()
            .filter(|name| name.starts_with('#'))
            .map(|name| name[1..].to_string())
            .collect::<Vec<String>>();
        match self.rng.below(if depth > 3 { 2 } else { 3 }) {
            0 if !lists.is_empty() => self.rng.pick(&lists).clone(),
            0 | 1 => {
                let count = self.rng.below(4);
                let elements = (0..count)
                    .map(|_| self.string_expression(scope, depth + 1))
                    .collect::<Vec<String>>();
                let ws = self.ows();
                let mut text = format!("[{ws}");
                for (i, element) in elements.iter().enumerate() {
                    if i > 0 {
                        text.push(',');
                        text.push_str(self.ows());
                    }
                    text.push_str(element);
                }
                if count > 0 && self.rng.chance(20) {
                    text.push(',');
                }
                text.push_str(self.ows());
                text.push(']');
                text
            }
            _ => {
                let inner = self.list_expression(scope, depth + 1);
                format!("({}{inner}{})", self.ows(), self.ows())
            }
        }
    }

    fn list_expression(&mut self, scope: &[String], depth: usize) -> String {
        match self.rng.below(if depth > 3 { 1 } else { 4 }) {
            0 => self.list_value(scope, depth),
            1 => self.conditional(scope, depth, Type::List),
            _ => {
                let lhs = self.list_value(scope, depth + 1);
                let rhs = self.list_expression(scope, depth + 1);
                format!("{lhs}{}++{}{rhs}", self.ows(), self.ows())
            }
        }
    }

    fn expression(&mut self, scope: &[String], depth: usize, kind: Type) -> String {
        match kind {
            Type::String => self.string_expression(scope, depth),
            Type::List => self.list_expression(scope, depth),
        }
    }

    fn comment(&mut self) -> String {
        let word = self.word();
        match self.rng.below(4) {
            0 => "#".to_string(),
            1 => format!("#{word}"),
            2 => format!("#   {word}   "),
            _ => format!("# {word}"),
        }
    }

    fn trailing_comment(&mut self) -> String {
        if self.rng.chance(15) {
            format!("{}{}", self.ws(), self.comment())
        } else if self.rng.chance(5) {
            self.ws().to_string()
        } else {
            String::new()
        }
    }

    fn assignment(&mut self, index: usize, kind: Type) -> String {
        let scope = (0..index)
            .map(|i| match self.variables[i] {
                Type::String => format!("v{i}"),
                Type::List => format!("#v{i}"),
            })
            .collect::<Vec<String>>();
        let value = self.expression(&scope, 0, kind);
        let mut text = String::new();
        if self.rng.chance(10) {
            text.push_str("[private]\n");
        }
        if kind == Type::String && self.rng.chance(15) {
            text.push_str("export");
            text.push_str(self.ws());
        }
        let ws1 = self.ows();
        let ws2 = self.ows();
        text.push_str(&format!("v{index}{ws1}:={ws2}{value}"));
        text
    }

    fn function(&mut self, index: usize) -> String {
        let arity = self.functions[index];
        let parameters = (0..arity).map(|i| format!("a{i}")).collect::<Vec<String>>();
        let body = self.string_expression(&parameters, 1);
        let mut text = format!("fun{index}{}(", self.ows());
        for (i, parameter) in parameters.iter().enumerate() {
            if i > 0 {
                text.push(',');
                text.push_str(self.ows());
            }
            text.push_str(parameter);
        }
        text.push_str(&format!("){}:={}{body}", self.ows(), self.ows()));
        text
    }

    fn setting(&mut self, name: &str) -> String {
        let boolean = !matches!(
            name,
            "dotenv-filename"
                | "dotenv-path"
                | "dotenv-command"
                | "tempdir"
                | "working-directory"
                | "indentation"
                | "minimum-version"
                | "shell"
                | "windows-shell"
                | "script-interpreter"
        );
        let ws1 = self.ws();
        if boolean {
            return match self.rng.below(3) {
                0 => format!("set{ws1}{name}"),
                1 => format!("set{ws1}{name}{}:={}true", self.ows(), self.ows()),
                _ => format!("set{ws1}{name} := false"),
            };
        }
        let value = match name {
            "indentation" => (*self
                .rng
                .pick(&["\" \"", "\"  \"", "'\t'", "\"\\t\"", "\"   \""]))
            .to_string(),
            "minimum-version" => {
                (*self.rng.pick(&["'1.0.0'", "\"1.58.0\"", "'0.9.9'"])).to_string()
            }
            "shell" | "windows-shell" | "script-interpreter" => {
                let count = 1 + self.rng.below(3);
                let mut text = String::from("[");
                for i in 0..count {
                    if i > 0 {
                        text.push(',');
                        text.push_str(self.ws());
                    }
                    text.push_str(&format!("'arg{i}'"));
                }
                if self.rng.chance(20) {
                    text.push(',');
                }
                text.push(']');
                text
            }
            _ => format!(
                "'{}'",
                self.rng.pick(&["dir", ".env.local", "/tmp", "cat .env"])
            ),
        };
        format!("set{ws1}{name}{}:={}{value}", self.ows(), self.ows())
    }

    fn render_attributes(&mut self, attributes: Vec<String>) -> String {
        let mut text = String::new();
        let mut index = 0;
        while index < attributes.len() {
            let count = 1 + self.rng.below(3).min(attributes.len() - index - 1);
            text.push('[');
            text.push_str(self.ows());
            for i in 0..count {
                if i > 0 {
                    text.push(',');
                    text.push_str(self.ows());
                }
                text.push_str(&attributes[index + i]);
            }
            text.push_str(self.ows());
            text.push(']');
            if self.rng.chance(10) {
                text.push_str(&self.trailing_comment());
            }
            text.push('\n');
            index += count;
        }
        text
    }

    fn recipe(&mut self, index: usize) -> String {
        let required = self.recipes[index].required;
        let optional = self.recipes[index].optional;
        let variadic = self.recipes[index].variadic;
        let referable = self.recipes[index].referable;

        let globals = (0..self.variables.len())
            .filter(|i| self.variables[*i] == Type::String)
            .map(|i| format!("v{i}"))
            .collect::<Vec<String>>();

        let mut attributes = Vec::new();
        let mut script = false;
        let shebang = self.rng.chance(12);

        if !referable {
            let platform = *self.rng.pick(&[
                "linux",
                "unix",
                "macos",
                "windows",
                "openbsd",
                "freebsd",
                "netbsd",
                "dragonfly",
                "android",
            ]);
            attributes.push(platform.to_string());
        }
        if self.rng.chance(20) {
            let group = self.rng.below(4);
            let form = if self.rng.chance(30) {
                format!("group: 'g{group}'")
            } else {
                format!("group('g{group}')")
            };
            attributes.push(form);
            if self.rng.chance(30) {
                attributes.push(format!("group(\"h{group}\")"));
            }
        }
        if self.rng.chance(10) {
            attributes.push("private".to_string());
        }
        if self.rng.chance(10) {
            let form = *self
                .rng
                .pick(&["confirm", "confirm('sure?')", "confirm: \"ok?\""]);
            attributes.push(form.to_string());
        }
        if self.rng.chance(8) {
            let form = *self.rng.pick(&["doc('documented')", "doc", "doc: \"x\""]);
            attributes.push(form.to_string());
        }
        if self.rng.chance(8) {
            if self.rng.chance(50) {
                attributes.push("no-cd".to_string());
            } else {
                attributes.push("working-directory('sub')".to_string());
            }
        }
        if self.rng.chance(6) {
            let form = *self.rng.pick(&["no-exit-message", "exit-message"]);
            attributes.push(form.to_string());
        }
        if self.rng.chance(6) {
            let form = *self
                .rng
                .pick(&["positional-arguments", "no-quiet", "parallel"]);
            attributes.push(form.to_string());
        }
        if self.rng.chance(6) {
            let key = self.rng.below(3);
            attributes.push(format!("env('K{key}', \"value\")"));
            if self.rng.chance(40) {
                attributes.push(format!("env(\"J{key}\", 'v2')"));
            }
        }
        if self.rng.chance(5) {
            let form = *self.rng.pick(&["metadata('m')", "metadata(\"a\", 'b')"]);
            attributes.push(form.to_string());
        }
        if self.rng.chance(4) {
            let form = *self.rng.pick(&[
                "continue",
                "continue('SIGINT')",
                "continue(\"SIGQUIT\", 'SIGHUP')",
            ]);
            attributes.push(form.to_string());
        }
        if !self.default_used && self.rng.chance(4) {
            self.default_used = true;
            attributes.push("default".to_string());
        }
        if !shebang && self.rng.chance(8) {
            script = true;
            let form = *self
                .rng
                .pick(&["script", "script('bash')", "script(\"sh\", '-eu')"]);
            attributes.push(form.to_string());
        }
        if (shebang || script) && self.rng.chance(20) {
            attributes.push("extension('.sh')".to_string());
        }
        if self.lists && (shebang || script) && self.rng.chance(15) {
            let form = *self.rng.pick(&[
                "cache",
                "cache(inputs='src')",
                "cache(outputs=\"out\", extra='x')",
            ]);
            attributes.push(form.to_string());
        }
        let parameter_names = (0..required + optional)
            .map(|i| format!("p{i}"))
            .collect::<Vec<String>>();
        if required > 0 && self.rng.chance(15) {
            let target = self.rng.below(required);
            let form = match self.rng.below(if self.lists { 6 } else { 4 }) {
                0 => format!("arg('p{target}', long)"),
                1 => format!("arg(\"p{target}\", short='s')"),
                2 => format!("arg('p{target}', long='opt', short, help='helps')"),
                3 => format!("arg('p{target}', pattern='[a-z]+')"),
                4 => format!("arg('p{target}', long, flag)"),
                _ => format!("arg('p{target}', long, value='v')"),
            };
            attributes.push(form);
        }

        let mut text = String::new();
        if self.rng.chance(15) {
            text.push_str(&self.comment());
            text.push('\n');
        }
        let attribute_count = attributes.len();
        let order = (0..attribute_count)
            .map(|_| self.rng.next())
            .collect::<Vec<u64>>();
        let mut shuffled = attributes
            .into_iter()
            .zip(order)
            .collect::<Vec<(String, u64)>>();
        shuffled.sort_by_key(|(_, key)| *key);
        let attributes = shuffled
            .into_iter()
            .map(|(attribute, _)| attribute)
            .collect();
        text.push_str(&self.render_attributes(attributes));

        if self.rng.chance(12) {
            text.push('@');
            text.push_str(self.ows());
        }
        text.push_str(&format!("r{index}"));

        let mut scope = globals.clone();
        for (i, name) in parameter_names.iter().enumerate() {
            text.push_str(self.ws());
            if self.rng.chance(10) {
                text.push('$');
            }
            text.push_str(name);
            if i >= required {
                let default = self.parameter_default(&scope);
                text.push_str(&format!("{}={}{default}", self.ows(), self.ows()));
            }
            scope.push(name.clone());
        }
        match variadic {
            Variadic::None => {}
            Variadic::Plus | Variadic::Star => {
                text.push_str(self.ws());
                text.push(if variadic == Variadic::Plus { '+' } else { '*' });
                text.push_str(self.ows());
                if self.rng.chance(10) {
                    text.push('$');
                }
                text.push_str("rest");
                if optional > 0 || self.rng.chance(20) {
                    let default = self.parameter_default(&scope);
                    text.push_str(&format!("={default}"));
                }
                scope.push("rest".to_string());
            }
        }
        text.push_str(self.ows());
        text.push(':');

        let candidates = (0..index)
            .filter(|i| self.recipes[*i].referable)
            .collect::<Vec<usize>>();
        let mut dependencies = Vec::new();
        if !candidates.is_empty() {
            for _ in 0..self.rng.below(3) {
                let target = *self.rng.pick(&candidates);
                dependencies.push(self.dependency(target, &scope));
            }
        }
        for dependency in &dependencies {
            text.push_str(self.ws());
            text.push_str(dependency);
        }
        if !candidates.is_empty() && self.rng.chance(15) {
            text.push_str(self.ows());
            text.push_str("&&");
            for _ in 0..1 + self.rng.below(2) {
                let target = *self.rng.pick(&candidates);
                let dependency = self.dependency(target, &scope);
                text.push_str(self.ws());
                text.push_str(&dependency);
            }
        }
        text.push_str(&self.trailing_comment());

        let lines = self.rng.below(5);
        if lines == 0 && !shebang {
            return text;
        }
        let indent = *self.rng.pick(INDENTS);
        text.push('\n');
        if shebang {
            text.push_str(indent);
            text.push_str(self.rng.pick(SHEBANGS));
            text.push('\n');
        }
        for line in 0..lines.max(1) {
            let choice = self.rng.below(12);
            let choice = if (line == 0 && (choice == 0 || choice == 8))
                || (choice == 8 && !(shebang || script))
            {
                9
            } else {
                choice
            };
            match choice {
                0 => {
                    if self.rng.chance(50) {
                        text.push_str(self.rng.pick(&["", " ", "\t", "  "]));
                    }
                }
                1 => {
                    text.push_str(indent);
                    text.push_str("# body comment");
                }
                2 => {
                    text.push_str(indent);
                    text.push_str("echo continued \\\n");
                    text.push_str(indent);
                    text.push_str("  more");
                }
                3 => {
                    text.push_str(indent);
                    text.push_str(self.rng.pick(&["@", "-", "@-", "-@"]));
                    text.push_str(self.rng.pick(BODY_TEXT));
                }
                4..=6 => {
                    let expression = self.string_expression(&scope, 2);
                    let ws1 = self.ows();
                    let ws2 = self.ows();
                    text.push_str(indent);
                    text.push_str(&format!("echo {{{{{ws1}{expression}{ws2}}}}} done"));
                }
                7 => {
                    text.push_str(indent);
                    text.push_str("echo {{{{ escaped }}");
                }
                8 => {
                    text.push_str(indent);
                    text.push_str("  ");
                    text.push_str(self.rng.pick(BODY_TEXT));
                }
                _ => {
                    text.push_str(indent);
                    text.push_str(self.rng.pick(BODY_TEXT));
                    if self.rng.chance(10) {
                        text.push_str("  ");
                    }
                }
            }
            text.push('\n');
        }
        if self.rng.chance(20) {
            text.push('\n');
        }
        text.pop();
        text
    }

    fn parameter_default(&mut self, scope: &[String]) -> String {
        let strings = scope
            .iter()
            .filter(|name| !name.starts_with('*'))
            .cloned()
            .collect::<Vec<String>>();
        match self.rng.below(5) {
            0 if !strings.is_empty() => self.rng.pick(&strings).clone(),
            1 => {
                let inner = self.string_expression(scope, 2);
                format!("({inner})")
            }
            2 => self.call(scope, 3),
            _ => self.string_literal(),
        }
    }

    fn dependency(&mut self, target: usize, scope: &[String]) -> String {
        let signature = &self.recipes[target];
        let minimum = signature.required + usize::from(signature.variadic == Variadic::Plus);
        let maximum = signature.required
            + signature.optional
            + if signature.variadic == Variadic::None {
                0
            } else {
                2
            };
        let count = minimum + self.rng.below(maximum - minimum + 1);
        if count == 0 && self.rng.chance(80) {
            return format!("r{target}");
        }
        let mut text = format!("({}r{target}", self.ows());
        for _ in 0..count {
            text.push_str(self.ws());
            let argument = loop {
                let argument = self.string_expression(scope, 2);
                if !argument.starts_with(['(', '/']) {
                    break argument;
                }
            };
            text.push_str(&argument);
        }
        text.push_str(self.ows());
        text.push(')');
        text
    }

    fn justfile(mut self) -> String {
        self.lists = self.rng.chance(35);

        let variable_count = self.rng.below(7);
        for _ in 0..variable_count {
            let kind = if self.lists && self.rng.chance(25) {
                Type::List
            } else {
                Type::String
            };
            self.variables.push(kind);
        }

        if self.lists {
            for _ in 0..self.rng.below(3) {
                let arity = self.rng.below(3);
                self.functions.push(arity);
            }
        }

        let recipe_count = 1 + self.rng.below(6);
        for _ in 0..recipe_count {
            let variadic = match self.rng.below(10) {
                _ if self.lists => Variadic::None,
                0 => Variadic::Plus,
                1 => Variadic::Star,
                _ => Variadic::None,
            };
            let signature = Signature {
                required: self.rng.below(3),
                optional: if self.rng.chance(30) {
                    1 + self.rng.below(2)
                } else {
                    0
                },
                variadic,
                referable: self.rng.chance(90),
            };
            self.recipes.push(signature);
        }

        let mut items = Vec::new();

        for index in 0..self.variables.len() {
            let kind = self.variables[index];
            items.push(self.assignment(index, kind));
        }

        for index in 0..self.functions.len() {
            items.push(self.function(index));
        }

        for index in 0..self.recipes.len() {
            items.push(self.recipe(index));
        }

        let referable = (0..self.recipes.len())
            .filter(|i| self.recipes[*i].referable)
            .collect::<Vec<usize>>();
        if !referable.is_empty() {
            for alias in 0..self.rng.below(3) {
                let target = *self.rng.pick(&referable);
                let private = if self.rng.chance(20) {
                    "[private]\n"
                } else {
                    ""
                };
                let ws1 = self.ws();
                let ws2 = self.ows();
                let ws3 = self.ows();
                items.push(format!(
                    "{private}alias{ws1}alias{alias}{ws2}:={ws3}r{target}"
                ));
            }
        }

        let mut settings = vec![
            "allow-duplicate-recipes",
            "allow-duplicate-variables",
            "default-list",
            "default-script",
            "export",
            "fallback",
            "guards",
            "ignore-comments",
            "lazy",
            "no-exit-message",
            "positional-arguments",
            "quiet",
            "windows-powershell",
            "tempdir",
            "indentation",
            "minimum-version",
            "shell",
            "windows-shell",
            "script-interpreter",
        ];
        settings.push(*self.rng.pick(&["no-cd", "working-directory"]));
        settings.push(*self.rng.pick(&[
            "dotenv-filename",
            "dotenv-path",
            "dotenv-command",
            "dotenv-override",
        ]));
        for name in settings {
            if self.rng.chance(8) {
                let mut text = String::new();
                if self.rng.chance(10) {
                    text.push_str("[unix]\n");
                }
                text.push_str(&self.setting(name));
                items.push(text);
            }
        }
        if self.lists {
            items.push(self.setting("lists").replace(" := false", ""));
            items.push("set unstable".to_string());
        } else if self.rng.chance(5) {
            items.push("set unstable".to_string());
        }

        for index in 0..self.rng.below(3) {
            let ws = self.ws();
            if self.rng.chance(50) {
                let path = *self.rng.pick(&[
                    "'missing.just'",
                    "\"other/missing.just\"",
                    "x'~/missing.just'",
                ]);
                items.push(format!("import?{ws}{path}"));
            } else {
                let mut text = String::new();
                if self.rng.chance(15) {
                    text.push_str("# module comment\n");
                }
                if self.rng.chance(25) {
                    text.push_str(self.rng.pick(&[
                        "[doc('module doc')]\n",
                        "[group('mods')]\n",
                        "[private]\n",
                    ]));
                }
                text.push_str(&format!("mod?{ws}m{index}"));
                if self.rng.chance(50) {
                    text.push_str(self.ws());
                    text.push_str(self.rng.pick(&["'missing/mod.just'", "\"m.just\""]));
                }
                items.push(text);
            }
        }

        for index in 0..self.rng.below(2) {
            items.push(format!("unexport{}U{index}", self.ws()));
        }

        let order = (0..items.len())
            .map(|_| self.rng.next())
            .collect::<Vec<u64>>();
        let mut shuffled = items.into_iter().zip(order).collect::<Vec<(String, u64)>>();
        shuffled.sort_by_key(|(_, key)| *key);

        let mut text = String::new();
        if self.rng.chance(5) {
            text.push_str("#!/usr/bin/env just --justfile\n");
        }
        for (i, (item, _)) in shuffled.into_iter().enumerate() {
            if i > 0 {
                for _ in 0..*self.rng.pick(&[0, 0, 0, 1, 1, 2]) {
                    if self.rng.chance(10) {
                        text.push_str(self.ws());
                    }
                    text.push('\n');
                }
            }
            if self.rng.chance(10) {
                text.push_str(&self.comment());
                text.push('\n');
                if self.rng.chance(30) {
                    text.push('\n');
                }
            }
            text.push_str(&item);
            let last_line = item.rsplit('\n').next().unwrap_or_default();
            if !last_line.is_empty() && !last_line.starts_with([' ', '\t']) {
                text.push_str(&self.trailing_comment());
            }
            text.push('\n');
        }
        match self.rng.below(10) {
            0 => {
                text.pop();
            }
            1 => text.push_str("\n\n"),
            _ => {}
        }
        if self.rng.chance(10) {
            text = text.replace('\n', "\r\n");
        }
        if self.rng.chance(4) {
            text.insert(0, '\u{feff}');
        }
        text
    }
}
