# eslint-plugin-makefmt

Format Makefiles and justfiles through ESLint with [makefmt](https://github.com/NazmusSayad/makefmt). Aligns assignments and line continuations, indents conditionals, and never touches recipes. Justfiles get the same output as `just --fmt`.

```make
CC=gcc
CFLAGS   :=-O2 -Wall

ifeq ($(DEBUG),1)
CFLAGS += -g
endif
```

becomes

```make
CC     = gcc
CFLAGS := -O2 -Wall

ifeq ($(DEBUG),1)
  CFLAGS += -g
endif
```

The formatter is written in Rust and runs inside ESLint as WebAssembly, so there is nothing else to install.

## Install

```sh
npm install --save-dev eslint eslint-plugin-makefmt
```

Requires ESLint 9.7 or later with flat config.

## Usage

```js
// eslint.config.js
import makefmt from 'eslint-plugin-makefmt'

export default [...makefmt.configs.recommended]
```

Then run `npx eslint .` to check and `npx eslint --fix .` to format.

The recommended config lints these files with the `makefmt/format` rule:

| Language | Files                                                            |
| -------- | ---------------------------------------------------------------- |
| Makefile | `Makefile`, `makefile`, `GNUmakefile`, `*.mk`, `*.mak`, `*.make` |
| justfile | `justfile`, `Justfile`, `.justfile`, `*.just`                    |

## Custom setup

Pick the language and the files yourself:

```js
import makefmt from 'eslint-plugin-makefmt'

export default [
  {
    files: ['**/*.makefile'],
    plugins: { makefmt },
    language: 'makefmt/makefile',
    rules: {
      'makefmt/format': ['error', { indentWidth: 4 }],
    },
  },
]
```

The plugin provides two languages, `makefmt/makefile` and `makefmt/justfile`, and one rule, `makefmt/format`. The rule reports files that are not formatted, and `--fix` formats them. Files that can't be formatted, such as a justfile with a syntax error, are reported with the error.

## Rule options

| Option                   | Default | Description                                         |
| ------------------------ | ------- | --------------------------------------------------- |
| `alignAssignments`       | `true`  | Align assignment operators of consecutive variables |
| `alignContinuations`     | `true`  | Align trailing backslashes of continued lines       |
| `indentConditionals`     | `true`  | Indent conditional blocks such as `ifeq`/`ifdef`    |
| `indentWidth`            | `2`     | Spaces per indentation level                        |
| `trimTrailingWhitespace` | `false` | Also trim trailing whitespace on assignment lines   |

Make keeps trailing whitespace in a variable's value, so `PORT := 2184   ` sets `PORT` to `2184   `. makefmt leaves it alone unless you turn on trimming, which can change values that relied on it.

The options only apply to Makefiles. Justfiles always use the style of `just --fmt`.

## Project config

A `.makefmt.toml` in the Makefile's folder or any parent folder takes precedence over the rule options, so ESLint, the CLI and the VS Code extension all produce the same output:

```toml
align_assignments = true
align_continuations = true
indent_conditionals = true
indent_width = 4
trim_trailing_whitespace = false
```

Missing keys use the defaults. Unknown keys are an error.

ESLint's `--cache` doesn't know about `.makefmt.toml`, so delete the cache file (`.eslintcache` by default) after changing it.

## oxlint

oxlint can load this plugin through `jsPlugins` and validates the rule options, but it can't lint Makefiles or justfiles yet: its JS plugins only run on JavaScript and TypeScript files, and custom languages are not supported. Use ESLint, or the [`makefmt`](https://www.npmjs.com/package/makefmt) CLI, for these files.
