# makefmt

Fast, opinionated Makefile formatter. Aligns assignments and line continuations and indents conditional blocks.

The formatter runs inside VS Code as WebAssembly, so formatting is instant and works on vscode.dev too.

## Usage

makefmt becomes the default formatter for Makefiles once installed. Run **Format Document** (`Shift+Alt+F`), or format on save:

```json
{
  "[makefile]": {
    "editor.formatOnSave": true
  }
}
```

## Settings

| Setting                      | Default | Description                                         |
| ---------------------------- | ------- | --------------------------------------------------- |
| `makefmt.alignAssignments`   | `true`  | Align assignment operators of consecutive variables |
| `makefmt.alignContinuations` | `true`  | Align trailing backslashes of continued lines       |
| `makefmt.indentConditionals` | `true`  | Indent conditional blocks such as `ifeq`/`ifdef`    |
| `makefmt.indentWidth`        | `2`     | Spaces per indentation level                        |

## Project config

A `.makefmt.toml` in the Makefile's folder or any parent folder takes precedence over the settings above, so everyone on the project gets the same output:

```toml
align_assignments = true
align_continuations = true
indent_conditionals = true
indent_width = 4
```

Missing keys use the defaults. Unknown keys are an error.

## CLI

The same formatter is available on npm as [`makefmt`](https://www.npmjs.com/package/makefmt) for CI and pre-commit hooks:

```sh
npx makefmt --check .
```
