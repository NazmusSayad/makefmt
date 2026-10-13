use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use libtest_mimic::{Arguments, Failed, Trial};
use makefmt::shfmt::{Lang, format};
use serde::{Deserialize, Serialize};

const SHFMT_VERSION: &str = "v3.14.1";
const BLESS_VARIABLE: &str = "SHFMT_FIXTURES_BLESS";
const MUTATION_CASES: u64 = 20000;
const FORMAT_STACK_SIZE: usize = 64 * 1024 * 1024;
const FORMAT_TIME_LIMIT: Duration = Duration::from_secs(10);
const LARGE_SCRIPT_COPIES: usize = 20;
const LANGS: &[&str] = &["bash", "posix"];
const INDENTS: &[usize] = &[0, 2, 4];
const ERROR_PREFIX: &str = "<standard input>:";

#[derive(Clone, Serialize, Deserialize)]
struct Case {
    input: String,
    lang: String,
    indent: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn main() {
    let arguments = Arguments::from_args();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/shfmt/cases.json");
    let cases: Vec<Case> = serde_json::from_str(&read(&path)).expect("cases.json is valid");

    if std::env::var_os(BLESS_VARIABLE).is_some() {
        let trial = Trial::test("bless", move || bless(&path, cases));
        libtest_mimic::run(&arguments, vec![trial]).exit();
    }

    let shfmt = Arc::new(check_shfmt());
    let cases = Arc::new(cases);
    let mut trials = (0..cases.len())
        .map(|index| {
            let cases = cases.clone();
            Trial::test(format!("fixture::{index:05}"), move || check(&cases[index]))
        })
        .collect::<Vec<Trial>>();
    trials.push(Trial::test("limits::nesting", check_nesting_limit));
    {
        let cases = cases.clone();
        let shfmt = shfmt.clone();
        trials.push(Trial::test("performance::large_script", move || {
            check_large_script(&cases, &shfmt)
        }));
    }
    for seed in 0..MUTATION_CASES {
        let cases = cases.clone();
        let shfmt = shfmt.clone();
        trials.push(Trial::test(format!("mutation::{seed:05}"), move || {
            check_mutation(seed, &cases, &shfmt)
        }));
    }
    libtest_mimic::run(&arguments, trials).exit();
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn lang(name: &str) -> Lang {
    match name {
        "bash" => Lang::Bash,
        "posix" => Lang::Posix,
        other => panic!("unknown lang {other}"),
    }
}

fn format_with_limit(case: &Case) -> Result<Result<String, String>, Failed> {
    let input = case.input.clone();
    let lang = lang(&case.lang);
    let indent = case.indent;
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .stack_size(FORMAT_STACK_SIZE)
        .spawn(move || sender.send(format(&input, lang, indent)))?;
    match receiver.recv_timeout(FORMAT_TIME_LIMIT) {
        Ok(result) => Ok(result),
        Err(RecvTimeoutError::Timeout) => Err(format!(
            "formatting took longer than {FORMAT_TIME_LIMIT:?}\ninput:\n{:?}",
            case.input
        )
        .into()),
        Err(RecvTimeoutError::Disconnected) => {
            Err(format!("formatting panicked\ninput:\n{:?}", case.input).into())
        }
    }
}

fn check(case: &Case) -> Result<(), Failed> {
    let actual = format_with_limit(case)?;
    let expected = match (&case.output, &case.error) {
        (Some(output), None) => Ok(output.clone()),
        (None, Some(error)) => Err(error.clone()),
        _ => return Err("fixture needs exactly one of output or error".into()),
    };
    if actual == expected {
        return Ok(());
    }
    Err(format!(
        "lang {} indent {}\ninput:\n{:?}\nexpected:\n{:?}\nactual:\n{:?}",
        case.lang, case.indent, case.input, expected, actual
    )
    .into())
}

fn check_large_script(cases: &[Case], shfmt: &Result<(), String>) -> Result<(), Failed> {
    shfmt.clone()?;
    let script = cases
        .iter()
        .filter(|case| case.lang == "bash" && case.indent == 0)
        .filter_map(|case| case.output.as_deref())
        .collect::<Vec<&str>>()
        .join("\n")
        .repeat(LARGE_SCRIPT_COPIES);
    let case = run_shfmt(Case {
        input: script,
        lang: "bash".to_string(),
        indent: 0,
        output: None,
        error: None,
    })?;
    check(&case)
}

fn check_nesting_limit() -> Result<(), Failed> {
    let deep = 100_000;
    let inputs = [
        format!("{}x{}", "(".repeat(deep), ")".repeat(deep)),
        format!("{}x{}", "$(".repeat(deep), ")".repeat(deep)),
        format!("(({}1{}))", "(".repeat(deep), ")".repeat(deep)),
        format!("(({}1))", "- ".repeat(deep)),
        format!("x{}", " && x".repeat(deep)),
        format!("x{}", " | x".repeat(deep)),
        format!("[[ {}x ]]", "! ".repeat(deep)),
        format!("{}x{}", "${a:-".repeat(deep), "}".repeat(deep)),
        format!("if x; then x;{} fi", " elif x; then x;".repeat(deep)),
    ];
    for input in inputs {
        let case = Case {
            input,
            lang: "bash".to_string(),
            indent: 0,
            output: None,
            error: None,
        };
        match format_with_limit(&case)? {
            Err(error) if error.ends_with("nesting is too deep") => {}
            other => {
                return Err(format!(
                    "expected a nesting error for {:?}..., got {other:?}",
                    &case.input[..40]
                )
                .into());
            }
        }
    }
    Ok(())
}

fn check_shfmt() -> Result<(), String> {
    let output = Command::new("shfmt").arg("--version").output().map_err(|error| {
        format!("differential tests require shfmt {SHFMT_VERSION} on PATH, but running `shfmt` failed: {error}")
    })?;
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version == SHFMT_VERSION {
        Ok(())
    } else {
        Err(format!(
            "differential tests require shfmt {SHFMT_VERSION} on PATH, but found `{version}`"
        ))
    }
}

fn check_mutation(seed: u64, cases: &[Case], shfmt: &Result<(), String>) -> Result<(), Failed> {
    shfmt.clone()?;
    let mut rng = Rng::new(seed);
    let mut input = rng.pick(cases).input.clone();
    if rng.chance(30) {
        input.push('\n');
        input.push_str(&rng.pick(cases).input);
    }
    let case = Case {
        input: mutate(&mut rng, &input),
        lang: rng.pick(LANGS).to_string(),
        indent: *rng.pick(INDENTS),
        output: None,
        error: None,
    };
    check(&run_shfmt(case)?)
}

fn bless(path: &PathBuf, cases: Vec<Case>) -> Result<(), Failed> {
    check_shfmt()?;
    let blessed = cases
        .into_iter()
        .map(run_shfmt)
        .collect::<Result<Vec<Case>, Failed>>()?;
    let mut json = serde_json::to_string_pretty(&blessed)?;
    json.push('\n');
    fs::write(path, json)?;
    Ok(())
}

fn run_shfmt(case: Case) -> Result<Case, Failed> {
    let mut child = Command::new("shfmt")
        .args(["-ln", &case.lang, "-i", &case.indent.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let Some(mut stdin) = child.stdin.take() else {
        return Err("shfmt stdin is unavailable".into());
    };
    stdin.write_all(case.input.as_bytes())?;
    drop(stdin);
    let output = child.wait_with_output()?;
    let (output, error) = if output.status.success() {
        (Some(String::from_utf8(output.stdout)?), None)
    } else {
        let stderr = String::from_utf8(output.stderr)?;
        let Some(error) = stderr.trim_end().strip_prefix(ERROR_PREFIX) else {
            return Err(format!("unexpected shfmt error: {stderr}").into());
        };
        (None, Some(error.to_string()))
    };
    Ok(Case {
        output,
        error,
        ..case
    })
}

const MUTATION_PIECES: &[&str] = &[
    "\n",
    "\t",
    " ",
    "  ",
    "\\",
    "\\\n",
    "\"",
    "'",
    "`",
    "$",
    "$(",
    "${",
    "$((",
    "$[",
    "$'",
    "$\"",
    "(",
    ")",
    "((",
    "))",
    "[",
    "]",
    "[[",
    "]]",
    "{",
    "}",
    ";",
    ";;",
    ";&",
    ";;&",
    "&",
    "&&",
    "|",
    "||",
    "|&",
    "!",
    "#",
    "# c\n",
    "=",
    "+=",
    "<",
    ">",
    ">>",
    "<<",
    "<<-",
    "<<<",
    "<<EOF\n",
    "\nEOF\n",
    "<<-EOF\n",
    "\n\tEOF\n",
    "<&",
    ">&",
    "&>",
    "<(",
    ">(",
    "*",
    "?",
    "@",
    "+(",
    "@(",
    "!(",
    "?(",
    "*(",
    ":",
    ":-",
    "#",
    "%",
    "/",
    "//",
    "^",
    ",",
    "if ",
    "then ",
    "elif ",
    "else ",
    "fi",
    "for ",
    "in ",
    "do ",
    "done",
    "while ",
    "until ",
    "case ",
    "esac",
    "function ",
    "select ",
    "time ",
    "coproc ",
    "let ",
    "declare ",
    "local ",
    "export ",
    "x",
    "a=b",
    "foo",
    "1",
    "\r\n",
    "é",
    "\n\n",
    "\n\t\t",
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
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ 0x7368_666d_7421_0001)
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
