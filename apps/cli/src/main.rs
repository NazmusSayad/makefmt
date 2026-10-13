use std::fs;
use std::io::{self, Read, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use ignore::WalkBuilder;
use makefmt::Options;

const VERSION: &str = match option_env!("MAKEFMT_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Parser)]
#[command(
    name = "makefmt",
    version = VERSION,
    about = "Fast, opinionated Makefile and justfile formatter"
)]
struct Cli {
    #[arg(help = "Files or directories to format; omit or pass - to read stdin")]
    paths: Vec<PathBuf>,
    #[arg(
        short,
        long,
        conflicts_with = "check",
        help = "Rewrite changed files in place"
    )]
    write: bool,
    #[arg(
        long,
        help = "List unformatted files and exit with status 1 if any are found"
    )]
    check: bool,
    #[arg(long, value_name = "N", help = "Spaces per indentation level")]
    indent_width: Option<usize>,
    #[arg(long, value_name = "N", help = "Most consecutive blank lines to keep")]
    max_blank_lines: Option<NonZeroUsize>,
    #[arg(long, value_name = "BOOL", help = "Align assignment operators")]
    align_assignments: Option<bool>,
    #[arg(
        long,
        value_name = "BOOL",
        help = "Keep aligning assignments across comment lines"
    )]
    align_across_comments: Option<bool>,
    #[arg(
        long,
        value_name = "BOOL",
        help = "Align line continuation backslashes"
    )]
    align_continuations: Option<bool>,
    #[arg(
        long,
        value_name = "BOOL",
        help = "Format recipe lines as shell scripts with shfmt rules"
    )]
    format_recipes: Option<bool>,
    #[arg(long, value_name = "BOOL", help = "Indent conditional blocks")]
    indent_conditionals: Option<bool>,
    #[arg(
        long,
        value_name = "BOOL",
        help = "Also trim trailing whitespace that make keeps in variable values"
    )]
    trim_trailing_whitespace: Option<bool>,
    #[arg(
        long,
        value_enum,
        help = "Format stdin and files passed by name as this language instead of detecting it from the file name; stdin defaults to makefile"
    )]
    language: Option<Language>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Language {
    Makefile,
    Justfile,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("makefmt: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: &Cli) -> Result<ExitCode, String> {
    let stdin = match cli.paths.as_slice() {
        [] => true,
        [path] => path.as_os_str() == "-",
        paths => {
            if paths.iter().any(|path| path.as_os_str() == "-") {
                return Err("- cannot be combined with other paths".to_string());
            }
            false
        }
    };
    if stdin {
        return run_stdin(cli);
    }

    let mut stdout = io::stdout().lock();
    let mut unformatted = false;
    let mut failed = false;
    for (file, language) in collect_files(&cli.paths, cli.language)? {
        let (source, formatted) = match format_file(&file, language, cli) {
            Ok(result) => result,
            Err(message) => {
                failed = true;
                eprintln!("makefmt: {}: {message}", file.display());
                continue;
            }
        };
        if cli.check {
            if formatted != source {
                unformatted = true;
                writeln!(stdout, "{}", file.display()).map_err(|error| error.to_string())?;
            }
        } else if cli.write {
            if formatted != source
                && let Err(error) = fs::write(&file, formatted)
            {
                failed = true;
                eprintln!("makefmt: {}: {error}", file.display());
            }
        } else {
            stdout
                .write_all(formatted.as_bytes())
                .map_err(|error| error.to_string())?;
        }
    }

    if failed {
        return Ok(ExitCode::from(2));
    }
    if unformatted {
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}

fn format_file(file: &Path, language: Language, cli: &Cli) -> Result<(String, String), String> {
    let source = fs::read_to_string(file).map_err(|error| error.to_string())?;
    let absolute = fs::canonicalize(file).map_err(|error| error.to_string())?;
    let directory = absolute
        .parent()
        .ok_or_else(|| "has no parent directory".to_string())?;
    let formatted = format_source(&source, language, directory, cli)?;
    Ok((source, formatted))
}

fn run_stdin(cli: &Cli) -> Result<ExitCode, String> {
    if cli.write {
        return Err("--write cannot be used with stdin".to_string());
    }
    let mut source = String::new();
    io::stdin()
        .read_to_string(&mut source)
        .map_err(|error| format!("stdin: {error}"))?;
    let directory = std::env::current_dir().map_err(|error| error.to_string())?;
    let language = cli.language.unwrap_or(Language::Makefile);
    let formatted = format_source(&source, language, &directory, cli)
        .map_err(|error| format!("stdin: {error}"))?;
    if cli.check {
        if formatted != source {
            eprintln!("stdin is not formatted");
            return Ok(ExitCode::from(1));
        }
        return Ok(ExitCode::SUCCESS);
    }
    io::stdout()
        .write_all(formatted.as_bytes())
        .map_err(|error| error.to_string())?;
    Ok(ExitCode::SUCCESS)
}

fn collect_files(
    paths: &[PathBuf],
    language: Option<Language>,
) -> Result<Vec<(PathBuf, Language)>, String> {
    let mut files = Vec::new();
    for path in paths {
        let metadata =
            fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
        if !metadata.is_dir() {
            let language = match language {
                Some(language) => language,
                None => detect_language(path).ok_or_else(|| {
                    format!(
                        "{}: not a Makefile or justfile name; pass --language",
                        path.display()
                    )
                })?,
            };
            files.push((path.clone(), language));
            continue;
        }
        let walker = WalkBuilder::new(path)
            .require_git(false)
            .hidden(false)
            .filter_entry(|entry| {
                let name = entry.file_name();
                !name.as_encoded_bytes().starts_with(b".") || name.eq_ignore_ascii_case(".justfile")
            })
            .sort_by_file_name(|a, b| a.cmp(b))
            .build();
        for entry in walker {
            let entry = entry.map_err(|error| error.to_string())?;
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            if let Some(language) = detect_language(entry.path()) {
                files.push((entry.into_path(), language));
            }
        }
    }
    Ok(files)
}

fn detect_language(path: &Path) -> Option<Language> {
    let name = path.file_name()?.to_str()?;
    let extension = path.extension().and_then(|extension| extension.to_str());
    if matches!(name, "Makefile" | "makefile" | "GNUmakefile")
        || matches!(extension, Some("mk" | "mak" | "make"))
    {
        return Some(Language::Makefile);
    }
    if name.eq_ignore_ascii_case("justfile")
        || name.eq_ignore_ascii_case(".justfile")
        || extension == Some("just")
    {
        return Some(Language::Justfile);
    }
    None
}

fn format_source(
    source: &str,
    language: Language,
    directory: &Path,
    cli: &Cli,
) -> Result<String, String> {
    match language {
        Language::Makefile => Ok(makefmt::format(source, &load_options(directory, cli)?)),
        Language::Justfile => makefmt::format_justfile(source),
    }
}

fn load_options(directory: &Path, cli: &Cli) -> Result<Options, String> {
    let config = directory
        .ancestors()
        .map(|ancestor| ancestor.join(".makefmt.toml"))
        .find(|candidate| candidate.is_file());
    let mut options = match config {
        Some(path) => {
            let text = fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            Options::from_toml(&text)
                .map_err(|error| format!("invalid config {}: {error}", path.display()))?
        }
        None => Options::default(),
    };
    if let Some(indent_width) = cli.indent_width {
        options.indent_width = indent_width;
    }
    if let Some(max_blank_lines) = cli.max_blank_lines {
        options.max_blank_lines = max_blank_lines;
    }
    if let Some(align_assignments) = cli.align_assignments {
        options.align_assignments = align_assignments;
    }
    if let Some(align_across_comments) = cli.align_across_comments {
        options.align_across_comments = align_across_comments;
    }
    if let Some(align_continuations) = cli.align_continuations {
        options.align_continuations = align_continuations;
    }
    if let Some(format_recipes) = cli.format_recipes {
        options.format_recipes = format_recipes;
    }
    if let Some(indent_conditionals) = cli.indent_conditionals {
        options.indent_conditionals = indent_conditionals;
    }
    if let Some(trim_trailing_whitespace) = cli.trim_trailing_whitespace {
        options.trim_trailing_whitespace = trim_trailing_whitespace;
    }
    Ok(options)
}
