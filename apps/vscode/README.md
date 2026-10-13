# makefmt

Fast, safe Makefile and justfile formatter. Aligns assignments and line continuations and indents conditional blocks.

```make
CC=gcc
CFLAGS   :=-O2 -Wall

ifeq ($(DEBUG),1)
CFLAGS += -g
endif

all:main.o   util.o
	$(CC) $(CFLAGS) -o app $^
```

becomes

```make
CC     = gcc
CFLAGS := -O2 -Wall

ifeq ($(DEBUG),1)
  CFLAGS += -g
endif

all: main.o util.o
	$(CC) $(CFLAGS) -o app $^
```

makefmt never changes what make does. Recipe lines and `define` bodies are left exactly as written.

Justfiles get the same output as `just --fmt`, without needing `just` installed.

The formatter is written in Rust and runs inside VS Code as WebAssembly, so formatting is instant and works on vscode.dev too.

## Usage

makefmt becomes the default formatter for Makefiles and justfiles once installed. Files named `justfile`, `Justfile`, `.justfile` or `*.just` are recognized as justfiles. Run **Format Document** (`Shift+Alt+F`), or format on save:

```json
{
  "[makefile]": {
    "editor.formatOnSave": true
  },
  "[just]": {
    "editor.formatOnSave": true
  }
}
```

## Settings

| Setting                          | Default | Description                                         |
| -------------------------------- | ------- | --------------------------------------------------- |
| `makefmt.alignAssignments`       | `true`  | Align assignment operators of consecutive variables |
| `makefmt.alignAcrossComments`    | `false` | Keep aligning across comment lines                  |
| `makefmt.alignContinuations`     | `true`  | Align trailing backslashes of continued lines       |
| `makefmt.indentConditionals`     | `true`  | Indent conditional blocks such as `ifeq`/`ifdef`    |
| `makefmt.indentWidth`            | `2`     | Spaces per indentation level                        |
| `makefmt.maxBlankLines`          | `1`     | Most consecutive blank lines to keep                |
| `makefmt.trimTrailingWhitespace` | `false` | Also trim trailing whitespace on assignment lines   |

Make keeps trailing whitespace in a variable's value, so `PORT := 2184   ` sets `PORT` to `2184   `. makefmt leaves it alone unless you turn on trimming, which can change values that relied on it.

## Project config

A `.makefmt.toml` in the Makefile's folder or any parent folder takes precedence over the settings above, so everyone on the project gets the same output:

```toml
align_across_comments = false
align_assignments = true
align_continuations = true
indent_conditionals = true
indent_width = 4
max_blank_lines = 1
trim_trailing_whitespace = false
```

Missing keys use the defaults. Unknown keys are an error. The settings and the file only apply to Makefiles; justfiles always use the style of `just --fmt`.

## CLI

The same formatter is available on npm as [`makefmt`](https://www.npmjs.com/package/makefmt) for CI and pre-commit hooks:

```sh
npx makefmt --check .
```
