import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { after, describe, test } from 'node:test'
import { RuleTester } from 'eslint'
import makefmt from 'eslint-plugin-makefmt'

RuleTester.describe = describe
RuleTester.it = test
RuleTester.itOnly = test.only

const ruleTester = new RuleTester({
  plugins: { makefmt },
  language: 'makefmt/justfile',
})

const cases: { source: string; output: string }[] = [
  {
    source:
      'x:="hello"\ny   :=   \'world\'\n\nbuild   target  *args:\n    echo {{target}} {{args}}\n',
    output:
      'x := "hello"\ny := \'world\'\n\nbuild target *args:\n    echo {{ target }} {{ args }}\n',
  },
  {
    source: 'alias   b:=build\n\nbuild:\n  cargo build\n',
    output: 'alias b := build\n\nbuild:\n    cargo build\n',
  },
  {
    source:
      'set   shell:=["bash","-cu"]\nset dotenv-load\n\ndefault:\n\t@just --list\n',
    output:
      'set shell := ["bash", "-cu"]\nset dotenv-load\n\ndefault:\n    @just --list\n',
  },
  {
    source: 'export   PATH:="bin:" + env_var("PATH")\n\nrun:\n    echo $PATH\n',
    output: 'export PATH := "bin:" + env_var("PATH")\n\nrun:\n    echo $PATH\n',
  },
  {
    source: 'foo:=if os()=="linux"{"a"}else{"b"}\n\nshow:\n    echo {{foo}}\n',
    output:
      'foo := if os() == "linux" { "a" } else { "b" }\n\nshow:\n    echo {{ foo }}\n',
  },
  {
    source: '[private]\n[no-cd]\nhelper   arg="x":\n    echo {{arg}}\n',
    output: '[no-cd]\n[private]\nhelper arg="x":\n    echo {{ arg }}\n',
  },
  {
    source:
      'test:   build   lint\n    cargo test\n\n\n\nbuild:\n    cargo build\nlint:\n    cargo clippy\n',
    output:
      'test: build lint\n    cargo test\n\nbuild:\n    cargo build\nlint:\n    cargo clippy\n',
  },
  {
    source: 'py:\n    #!/usr/bin/env python3\n    print("hi")\n',
    output: 'py:\n    #!/usr/bin/env python3\n    print("hi")\n',
  },
  {
    source:
      '# a comment\nversion   :=   `git describe`\n\nrelease $TAG=version:\n    echo $TAG\n',
    output:
      '# a comment\nversion := `git describe`\n\nrelease $TAG=version:\n    echo $TAG\n',
  },
  {
    source: 'a:\n    echo a\n\n\nb: a\n    echo b\n',
    output: 'a:\n    echo a\n\nb: a\n    echo b\n',
  },
  {
    source: 'x := "1"\n\nbuild:\n    echo {{ x }}\n',
    output: 'x := "1"\n\nbuild:\n    echo {{ x }}\n',
  },
  {
    source: 'default:\n    @echo hi\n',
    output: 'default:\n    @echo hi\n',
  },
]

const root = mkdtempSync(path.join(os.tmpdir(), 'eslint-plugin-makefmt-'))
after(() => rmSync(root, { recursive: true, force: true }))
mkdirSync(path.join(root, 'project'))
writeFileSync(
  path.join(root, 'project', '.makefmt.toml'),
  'this is not toml = = =\n'
)

const valid: RuleTester.ValidTestCase[] = []
const invalid: RuleTester.InvalidTestCase[] = []
for (const testCase of cases) {
  if (!valid.some((validCase) => validCase.code === testCase.output)) {
    valid.push({ code: testCase.output })
    valid.push({
      code: testCase.output,
      options: [{ indentWidth: 8, alignAssignments: false }],
    })
    valid.push({
      code: testCase.output,
      filename: path.join(root, 'project', 'justfile'),
    })
  }
  if (testCase.source !== testCase.output) {
    invalid.push({
      code: testCase.source,
      output: testCase.output,
      errors: [{ messageId: 'unformatted' }],
    })
    invalid.push({
      code: testCase.source,
      options: [{ indentWidth: 8, indentConditionals: false }],
      output: testCase.output,
      errors: [{ messageId: 'unformatted' }],
    })
    invalid.push({
      code: testCase.source,
      filename: path.join(root, 'project', 'sub', 'justfile'),
      output: testCase.output,
      errors: [{ messageId: 'unformatted' }],
    })
  }
}

ruleTester.run('format justfiles', makefmt.rules.format, { valid, invalid })

ruleTester.run('format justfile errors', makefmt.rules.format, {
  valid: [],
  invalid: [
    {
      code: 'build:\necho\n  x := \n',
      output: null,
      errors: [{ message: /^makefmt: .+/, line: 1, column: 1 }],
    },
    {
      code: 'foo := "unterminated\n',
      output: null,
      errors: [{ message: /^makefmt: .+/, line: 1, column: 1 }],
    },
    {
      code: 'foo := "unterminated\n',
      filename: path.join(root, 'project', 'justfile'),
      output: null,
      errors: [{ messageId: 'formatError' }],
    },
  ],
})
