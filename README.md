<img src="https://raw.githubusercontent.com/NazmusSayad/makefmt/main/apps/vscode/icon.png" alt="" width="96" align="right">

# makefmt

A fast, safe Makefile and justfile formatter, for the command line, VS Code and ESLint.

[VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=NazmusSayad.makefmt) · [Open VSX](https://open-vsx.org/extension/NazmusSayad/makefmt) · [npm](https://www.npmjs.com/package/makefmt) · [ESLint plugin](https://www.npmjs.com/package/eslint-plugin-makefmt)

The formatter is written in Rust. The CLI ships as a native binary, and the VS Code extension and the ESLint plugin run the same code as WebAssembly, so format on save is instant.

```make
CC=gcc
CFLAGS   :=-O2 -Wall
PREFIX?=/usr/local

SRCS = main.c \
	util.c \
      parser.c

ifeq ($(DEBUG),1)
CFLAGS += -g
endif

all:$(SRCS:.c=.o)   lib.a
	$(CC) $(CFLAGS) -o app $^
```

becomes

```make
CC     = gcc
CFLAGS := -O2 -Wall
PREFIX ?= /usr/local

SRCS = main.c \
  util.c      \
  parser.c

ifeq ($(DEBUG),1)
  CFLAGS += -g
endif

all: $(SRCS:.c=.o) lib.a
	$(CC) $(CFLAGS) -o app $^
```

makefmt never changes what make does. Recipe lines and `define` bodies are left exactly as written, and anything it can't format safely stays as it is.

The one thing it can't see is a `.RECIPEPREFIX` set outside the file, in an included Makefile, on the command line or in the environment. Set `.RECIPEPREFIX` in the Makefile that uses it.

Justfiles get the same output as `just --fmt`, without needing `just` installed. makefmt contains a port of just's own parser and printer, and is tested against just 1.58.0.

A few rare cases differ from `just --fmt`: makefmt doesn't expand environment variables in `x'...'` strings while parsing, it rejects expressions nested more than about 1,000 levels deep, and in VS Code and ESLint it treats OS-specific settings such as `[unix] set indentation` as disabled.

## Quick start

```sh
npx makefmt --write .
```

For VS Code, install the extension from the [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=NazmusSayad.makefmt), or from [Open VSX](https://open-vsx.org/extension/NazmusSayad/makefmt) for VSCodium, Cursor and Windsurf. It becomes the default formatter for Makefiles and justfiles, so **Format Document** and format on save just work.

For ESLint, see [`eslint-plugin-makefmt`](https://github.com/NazmusSayad/makefmt/tree/main/apps/eslint-plugin#readme).

Runs on macOS (Apple Silicon and Intel), Linux (x64 and arm64) and Windows (x64).

## Usage

```sh
makefmt [OPTIONS] [PATHS]...
```

With no paths, or `-`, makefmt formats stdin to stdout as a Makefile. Directories are searched for Makefiles (`Makefile`, `makefile`, `GNUmakefile`, `*.mk`, `*.mak`, `*.make`) and justfiles (`justfile` in any case, `.justfile`, `*.just`), skipping other hidden files and folders and anything in `.gitignore`.

| Option                              | Description                                                         |
| ----------------------------------- | ------------------------------------------------------------------- |
| `-w`, `--write`                     | Rewrite files in place                                              |
| `--check`                           | List unformatted files and exit with 1 if there are any             |
| `--indent-width <N>`                | Spaces per indentation level (default `2`)                          |
| `--max-blank-lines <N>`             | Most consecutive blank lines to keep (default `1`)                  |
| `--align-across-comments <BOOL>`    | Keep aligning across comment lines (default `false`)                |
| `--align-assignments <BOOL>`        | Align `=`, `:=`, `?=`, `+=` (default `true`)                        |
| `--align-continuations <BOOL>`      | Align trailing `\` (default `true`)                                 |
| `--indent-conditionals <BOOL>`      | Indent `ifeq`/`ifdef` blocks (default `true`)                       |
| `--trim-trailing-whitespace <BOOL>` | Also trim trailing whitespace on assignment lines (default `false`) |
| `--language <LANGUAGE>`             | Format stdin and named files as `makefile` or `justfile`            |

Exit codes: `0` success, `1` `--check` found unformatted files, `2` error.

## Configuration

Put a `.makefmt.toml` in your project. The CLI, the VS Code extension and the ESLint plugin all use the nearest one above the Makefile:

```toml
align_across_comments = false
align_assignments = true
align_continuations = true
indent_conditionals = true
indent_width = 2
max_blank_lines = 1
trim_trailing_whitespace = false
```

CLI flags override the file. In VS Code and ESLint, the file overrides the `makefmt.*` settings and rule options. These options only apply to Makefiles; justfiles always use the style of `just --fmt`.

Trailing whitespace is removed everywhere except on variable assignments, because make keeps it as part of the value: `PORT := 2184   ` sets `PORT` to `2184   `, so `$(PORT)/db` becomes `2184   /db`. If that whitespace is never intended in your project, set `trim_trailing_whitespace = true`. Lines where trimming would change more than the value, such as one ending in `\   `, are still left alone.

## Development

Requires Node.js, pnpm and Rust, plus the WebAssembly target: `rustup target add wasm32-unknown-unknown`.

```sh
pnpm install
pnpm build
pnpm lint
pnpm test
```

The tests compare against the real tools, so `pnpm test` needs [just](https://github.com/casey/just) 1.58.0 and GNU make 4.4 on `PATH`.

| Path                 | What it is                                             |
| -------------------- | ------------------------------------------------------ |
| `crates/core`        | The Makefile and justfile formatters                   |
| `crates/wasm`        | WebAssembly build of the formatter, shared by the apps |
| `apps/cli`           | The `makefmt` binary                                   |
| `apps/vscode`        | The VS Code extension                                  |
| `apps/eslint-plugin` | `eslint-plugin-makefmt`                                |
| `scripts`            | Builds the npm package around the binaries             |

`pnpm --filter ./apps/vscode package` builds `apps/vscode/makefmt.vsix`, which you can install with **Extensions: Install from VSIX...**.

## Releasing

Publish a GitHub release tagged `vX.Y.Z`. The workflow runs the tests, builds the binaries, publishes `makefmt` and `eslint-plugin-makefmt` to npm and the extension to the VS Code Marketplace and Open VSX, and attaches the binaries and the `.vsix` to the release. Pre-releases only go to npm, under the `next` tag.
