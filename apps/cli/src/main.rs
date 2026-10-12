use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
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
    about = "Fast, opinionated Makefile formatter"
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
    #[arg(long, value_name = "BOOL", help = "Align assignment operators")]
    align_assignments: Option<bool>,
    #[arg(
        long,
        value_name = "BOOL",
        help = "Align line continuation backslashes"
    )]
    align_continuations: Option<bool>,
    #[arg(long, value_name = "BOOL", help = "Indent conditional blocks")]
    indent_conditionals: Option<bool>,
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
    for file in collect_files(&cli.paths)? {
        let source =
            fs::read_to_string(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        let absolute =
            fs::canonicalize(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        let directory = absolute
            .parent()
            .ok_or_else(|| format!("{}: has no parent directory", file.display()))?;
        let formatted = makefmt::format(&source, &load_options(directory, cli)?);
        if cli.check {
            if formatted != source {
                unformatted = true;
                writeln!(stdout, "{}", file.display()).map_err(|error| error.to_string())?;
            }
        } else if cli.write {
            if formatted != source {
                fs::write(&file, formatted)
                    .map_err(|error| format!("{}: {error}", file.display()))?;
            }
        } else {
            stdout
                .write_all(formatted.as_bytes())
                .map_err(|error| error.to_string())?;
        }
    }

    if unformatted {
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
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
    let formatted = makefmt::format(&source, &load_options(&directory, cli)?);
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

fn collect_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for path in paths {
        let metadata =
            fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
        if !metadata.is_dir() {
            files.push(path.clone());
            continue;
        }
        let walker = WalkBuilder::new(path)
            .require_git(false)
            .sort_by_file_name(|a, b| a.cmp(b))
            .build();
        for entry in walker {
            let entry = entry.map_err(|error| error.to_string())?;
            if entry.file_type().is_some_and(|kind| kind.is_file()) && is_makefile(entry.path()) {
                files.push(entry.into_path());
            }
        }
    }
    Ok(files)
}

fn is_makefile(path: &Path) -> bool {
    let name = path.file_name().and_then(|name| name.to_str());
    let extension = path.extension().and_then(|extension| extension.to_str());
    matches!(name, Some("Makefile" | "makefile" | "GNUmakefile"))
        || matches!(extension, Some("mk" | "mak" | "make"))
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
    if let Some(align_assignments) = cli.align_assignments {
        options.align_assignments = align_assignments;
    }
    if let Some(align_continuations) = cli.align_continuations {
        options.align_continuations = align_continuations;
    }
    if let Some(indent_conditionals) = cli.indent_conditionals {
        options.indent_conditionals = indent_conditionals;
    }
    Ok(options)
}
