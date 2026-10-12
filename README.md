<img src="https://raw.githubusercontent.com/NazmusSayad/makefmt/main/apps/vscode/icon.png" alt="" width="96" align="right">

# makefmt

A fast, safe Makefile formatter, for the command line and VS Code.

[VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=NazmusSayad.makefmt) · [Open VSX](https://open-vsx.org/extension/NazmusSayad/makefmt) · [npm](https://www.npmjs.com/package/makefmt)

The formatter is written in Rust. The CLI ships as a native binary, and the VS Code extension runs the same code as WebAssembly inside the editor, so format on save is instant.

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

## Quick start

```sh
npx makefmt --write .
```

For VS Code, install the extension from the [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=NazmusSayad.makefmt), or from [Open VSX](https://open-vsx.org/extension/NazmusSayad/makefmt) for VSCodium, Cursor and Windsurf. It becomes the default formatter for Makefiles, so **Format Document** and format on save just work.

Runs on macOS (Apple Silicon and Intel), Linux (x64 and arm64) and Windows (x64).

## Usage

```sh
makefmt [OPTIONS] [PATHS]...
```

With no paths, or `-`, makefmt formats stdin to stdout. Directories are searched for `Makefile`, `makefile`, `GNUmakefile`, `*.mk`, `*.mak` and `*.make`, skipping anything in `.gitignore`.

| Option                         | Description                                             |
| ------------------------------ | ------------------------------------------------------- |
| `-w`, `--write`                | Rewrite files in place                                  |
| `--check`                      | List unformatted files and exit with 1 if there are any |
| `--indent-width <N>`           | Spaces per indentation level (default `2`)              |
| `--align-assignments <BOOL>`   | Align `=`, `:=`, `?=`, `+=` (default `true`)            |
| `--align-continuations <BOOL>` | Align trailing `\` (default `true`)                     |
| `--indent-conditionals <BOOL>` | Indent `ifeq`/`ifdef` blocks (default `true`)           |

Exit codes: `0` success, `1` `--check` found unformatted files, `2` error.

## Configuration

Put a `.makefmt.toml` in your project. The CLI and the VS Code extension both use the nearest one above the Makefile:

```toml
align_assignments = true
align_continuations = true
indent_conditionals = true
indent_width = 2
```

CLI flags override the file. In VS Code, the file overrides the `makefmt.*` settings.

## Development

Requires Node.js, pnpm and Rust, plus the WebAssembly target: `rustup target add wasm32-unknown-unknown`.

```sh
pnpm install
pnpm build
pnpm lint
```

| Path          | What it is                                     |
| ------------- | ---------------------------------------------- |
| `crates/core` | The formatter                                  |
| `apps/cli`    | The `makefmt` binary                           |
| `apps/vscode` | The VS Code extension and its WebAssembly glue |
| `scripts`     | Builds the npm package around the binaries     |

`pnpm --filter ./apps/vscode package` builds `apps/vscode/makefmt.vsix`, which you can install with **Extensions: Install from VSIX...**.

## Releasing

Publish a GitHub release tagged `vX.Y.Z`. The workflow builds the binaries, publishes `makefmt` to npm and the extension to the VS Code Marketplace and Open VSX, and attaches the binaries and the `.vsix` to the release. Pre-releases only go to npm, under the `next` tag.
