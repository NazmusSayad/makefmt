import assert from 'node:assert/strict'
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { after, describe, test } from 'node:test'
import { loadFormatter } from '@makefmt/wasm'
import { Linter, RuleTester } from 'eslint'
import makefmt from 'eslint-plugin-makefmt'

RuleTester.describe = describe
RuleTester.it = test
RuleTester.itOnly = test.only

const format = loadFormatter(
  readFileSync(new URL('../dist/makefmt.wasm', import.meta.url))
)

const ruleTester = new RuleTester({
  plugins: { makefmt },
  language: 'makefmt/makefile',
})

const readme = [
  'CC=gcc',
  'CFLAGS   :=-O2 -Wall',
  'PREFIX?=/usr/local',
  '',
  'SRCS = main.c \\',
  '\tutil.c \\',
  '      parser.c',
  '',
  'ifeq ($(DEBUG),1)',
  'CFLAGS += -g',
  'endif',
  '',
  'all:$(SRCS:.c=.o)   lib.a',
  '\t$(CC) $(CFLAGS) -o app $^',
  '',
].join('\n')

const readmeFormatted = [
  'CC     = gcc',
  'CFLAGS := -O2 -Wall',
  'PREFIX ?= /usr/local',
  '',
  'SRCS = main.c \\',
  '  util.c      \\',
  '  parser.c',
  '',
  'ifeq ($(DEBUG),1)',
  '  CFLAGS += -g',
  'endif',
  '',
  'all: $(SRCS:.c=.o) lib.a',
  '\t$(CC) $(CFLAGS) -o app $^',
  '',
].join('\n')

const sources = [
  readme,
  'CC=gcc\nCXX  =  g++\nLONG_NAME:=1\nX?=2\nY+=3\n',
  'SRCS = a.c \\\n\tbb.c \\\n      ccc.c\n\nOBJS := $(SRCS:.c=.o) \\\n  extra.o\n',
  'ifeq ($(OS),Windows_NT)\nEXT=.exe\nelse\nifdef DEBUG\nCFLAGS+=-g\nendif\nEXT=\nendif\n',
  'define HELP\n  usage  =  make\nendef\nexport HELP\nA=1\nBB=2\n',
  '.PHONY: all clean\nall:main.o   util.o\n\t$(CC) -o app $^\nclean:\n\trm -f *.o\n',
  'NAME=héllo 🎉\nX  :=  1\nifneq ($(NAME),)\nY=ümläut\nendif\n',
  'all:\n\techo done',
  '',
  '\n\n\n',
  'CC=gcc\r\nX  :=  1\r\n',
  'PORT := 2184   \nURL = http://x:$(PORT)/db\t\nall:\n\techo hi   \n',
]

const alignAssignmentsValues = [undefined, true, false]
const alignContinuationsValues = [undefined, true, false]
const indentConditionalsValues = [undefined, true, false]
const indentWidthValues = [undefined, 0, 4]
const trimTrailingWhitespaceValues = [undefined, true, false]

interface FormatOptions {
  alignAssignments?: boolean
  alignContinuations?: boolean
  indentConditionals?: boolean
  indentWidth?: number
  trimTrailingWhitespace?: boolean
}

const optionSets: { options: FormatOptions; config: string }[] = []
for (const alignAssignments of alignAssignmentsValues) {
  for (const alignContinuations of alignContinuationsValues) {
    for (const indentConditionals of indentConditionalsValues) {
      for (const indentWidth of indentWidthValues) {
        for (const trimTrailingWhitespace of trimTrailingWhitespaceValues) {
          const options: FormatOptions = {}
          const config = []
          if (alignAssignments !== undefined) {
            options.alignAssignments = alignAssignments
            config.push(`align_assignments = ${alignAssignments}`)
          }
          if (alignContinuations !== undefined) {
            options.alignContinuations = alignContinuations
            config.push(`align_continuations = ${alignContinuations}`)
          }
          if (indentConditionals !== undefined) {
            options.indentConditionals = indentConditionals
            config.push(`indent_conditionals = ${indentConditionals}`)
          }
          if (indentWidth !== undefined) {
            options.indentWidth = indentWidth
            config.push(`indent_width = ${indentWidth}`)
          }
          if (trimTrailingWhitespace !== undefined) {
            options.trimTrailingWhitespace = trimTrailingWhitespace
            config.push(`trim_trailing_whitespace = ${trimTrailingWhitespace}`)
          }
          optionSets.push({ options, config: config.join('\n') })
        }
      }
    }
  }
}

const tableSources = [
  ...sources,
  ...sources.map((source) => format(source, '', 'makefile')),
].filter((source, index, all) => all.indexOf(source) === index)

const tableValid: RuleTester.ValidTestCase[] = []
const tableInvalid: RuleTester.InvalidTestCase[] = []
for (const source of tableSources) {
  for (const optionSet of optionSets) {
    const output = format(source, optionSet.config, 'makefile')
    if (output === source) {
      tableValid.push({ code: source, options: [optionSet.options] })
    } else {
      tableInvalid.push({
        code: source,
        options: [optionSet.options],
        output,
        errors: [{ messageId: 'unformatted' }],
      })
    }
  }
}

test('the generated table has both valid and invalid cases', () => {
  assert.ok(tableValid.length > 100)
  assert.ok(tableInvalid.length > 100)
})

ruleTester.run('format (generated option table)', makefmt.rules.format, {
  valid: tableValid,
  invalid: tableInvalid,
})

ruleTester.run('format', makefmt.rules.format, {
  valid: [
    readmeFormatted,
    '',
    'X = 1\n',
    { code: 'X = 1\n', filename: 'Makefile' },
    { code: 'X = 1\n', filename: 'rules.mk' },
    {
      code: 'ifdef A\nX = 1\nendif\n',
      options: [{ indentConditionals: false }],
    },
    { code: 'ifdef A\n    X = 1\nendif\n', options: [{ indentWidth: 4 }] },
    { code: 'A = 1\nBB = 2\n', options: [{ alignAssignments: false }] },
    {
      code: 'SRCS = a.c \\\n  bb.c \\\n  c.c\n',
      options: [{ alignContinuations: false }],
    },
    {
      code: 'ifdef A\n\tifdef B\n\t\tX = 1\n\tendif\nendif\n',
      options: [{ indentWidth: 0 }],
    },
    'all:\n\t  echo   "recipes are never touched"\n',
    'define F\n  a  =  b\nendef\n',
    'PORT := 2184   \n',
    { code: 'PORT := 2184   \n', options: [{ trimTrailingWhitespace: false }] },
    { code: 'K = a \\   \n', options: [{ trimTrailingWhitespace: true }] },
  ],
  invalid: [
    {
      code: 'PORT := 2184   \nQ = " a "\t\n',
      options: [{ trimTrailingWhitespace: true }],
      output: 'PORT := 2184\nQ    = " a "\n',
      errors: [{ messageId: 'unformatted' }],
    },
    {
      code: readme,
      output: readmeFormatted,
      errors: [
        {
          messageId: 'unformatted',
          line: 1,
          column: 3,
          endLine: 13,
          endColumn: 20,
        },
      ],
    },
    {
      code: 'CC=gcc\n',
      output: 'CC = gcc\n',
      errors: [
        {
          message: 'File is not formatted',
          line: 1,
          column: 3,
          endLine: 1,
          endColumn: 4,
        },
      ],
    },
    {
      code: 'all:\n\techo done',
      output: 'all:\n\techo done\n',
      errors: [
        {
          messageId: 'unformatted',
          line: 2,
          column: 11,
          endLine: 2,
          endColumn: 11,
        },
      ],
    },
    {
      code: 'X = 1\n\n\n\nY = 2\n',
      output: 'X = 1\n\nY = 2\n',
      errors: [
        {
          messageId: 'unformatted',
          line: 3,
          column: 1,
          endLine: 5,
          endColumn: 1,
        },
      ],
    },
    {
      code: '\n\n\n',
      output: '',
      errors: [
        {
          messageId: 'unformatted',
          line: 1,
          column: 1,
          endLine: 4,
          endColumn: 1,
        },
      ],
    },
    {
      code: 'NAME=🎉\nX  :=  1\n',
      output: 'NAME = 🎉\nX    := 1\n',
      errors: [
        {
          messageId: 'unformatted',
          line: 1,
          column: 5,
          endLine: 2,
          endColumn: 7,
        },
      ],
    },
    {
      code: 'ifdef A\nX = 1\nendif\n',
      output: 'ifdef A\n  X = 1\nendif\n',
      errors: [{ messageId: 'unformatted', line: 2, column: 1 }],
    },
    {
      code: 'ifdef A\nX = 1\nendif\n',
      options: [{ indentWidth: 4 }],
      output: 'ifdef A\n    X = 1\nendif\n',
      errors: [{ messageId: 'unformatted' }],
    },
    {
      code: 'ifdef A\n  X = 1\nendif\n',
      options: [{ indentConditionals: false }],
      output: 'ifdef A\nX = 1\nendif\n',
      errors: [{ messageId: 'unformatted' }],
    },
    {
      code: 'A  = 1\nBB = 2\n',
      options: [{ alignAssignments: false }],
      output: 'A = 1\nBB = 2\n',
      errors: [{ messageId: 'unformatted' }],
    },
    {
      code: 'SRCS = a.c  \\\n  bb.c \\\n  c.c\n',
      options: [{ alignContinuations: false }],
      output: 'SRCS = a.c \\\n  bb.c \\\n  c.c\n',
      errors: [{ messageId: 'unformatted' }],
    },
  ],
})

describe('rule options schema', () => {
  const invalidOptions = [
    { bogus: true },
    { align_assignments: true },
    { alignAssignments: 'yes' },
    { alignContinuations: 1 },
    { indentConditionals: null },
    { indentWidth: -1 },
    { indentWidth: 1.5 },
    { indentWidth: '2' },
  ]
  for (const options of invalidOptions) {
    test(`rejects ${JSON.stringify(options)}`, () => {
      const linter = new Linter()
      assert.throws(
        () =>
          linter.verify(
            'X = 1\n',
            {
              files: ['**/Makefile'],
              plugins: { makefmt },
              language: 'makefmt/makefile',
              rules: { 'makefmt/format': ['error', options] },
            },
            'Makefile'
          ),
        /makefmt\/format/
      )
    })
  }

  test('rejects more than one option object', () => {
    const linter = new Linter()
    assert.throws(
      () =>
        linter.verify(
          'X = 1\n',
          {
            files: ['**/Makefile'],
            plugins: { makefmt },
            language: 'makefmt/makefile',
            rules: { 'makefmt/format': ['error', {}, {}] },
          },
          'Makefile'
        ),
      /makefmt\/format/
    )
  })
})

const root = mkdtempSync(path.join(os.tmpdir(), 'eslint-plugin-makefmt-'))
after(() => rmSync(root, { recursive: true, force: true }))

function writeFile(relativePath: string, text: string) {
  const filePath = path.join(root, relativePath)
  mkdirSync(path.dirname(filePath), { recursive: true })
  writeFileSync(filePath, text)
  return filePath
}

writeFile('width4/.makefmt.toml', 'indent_width = 4\n')
writeFile('width4/nested/width8/.makefmt.toml', 'indent_width = 8\n')
writeFile('empty/.makefmt.toml', '')
writeFile('no-align/.makefmt.toml', 'align_assignments = false\n')
writeFile('invalid/.makefmt.toml', 'indent_width = "four"\n')
writeFile('unknown/.makefmt.toml', 'tab_width = 4\n')
writeFile('broken/.makefmt.toml', 'indent_width = \n')
mkdirSync(path.join(root, 'none'))

const conditional = 'ifdef A\nX = 1\nendif\n'

ruleTester.run('format with .makefmt.toml', makefmt.rules.format, {
  valid: [
    {
      code: 'ifdef A\n    X = 1\nendif\n',
      filename: path.join(root, 'width4', 'Makefile'),
    },
    {
      code: 'ifdef A\n    X = 1\nendif\n',
      filename: path.join(root, 'width4', 'a', 'b', 'c', 'rules.mk'),
    },
    {
      code: 'ifdef A\n        X = 1\nendif\n',
      filename: path.join(root, 'width4', 'nested', 'width8', 'Makefile'),
    },
    {
      code: 'ifdef A\n        X = 1\nendif\n',
      filename: path.join(root, 'width4', 'nested', 'width8', 'x', 'Makefile'),
    },
    {
      code: 'ifdef A\n    X = 1\nendif\n',
      filename: path.join(root, 'width4', 'nested', 'Makefile'),
      options: [{ indentWidth: 2, indentConditionals: false }],
    },
    {
      code: 'ifdef A\n  X = 1\nendif\n',
      filename: path.join(root, 'empty', 'Makefile'),
      options: [{ indentWidth: 4 }],
    },
    {
      code: 'A = 1\nBB = 2\n',
      filename: path.join(root, 'no-align', 'Makefile'),
    },
    {
      code: 'ifdef A\n    X = 1\nendif\n',
      filename: path.join(root, 'none', 'Makefile'),
      options: [{ indentWidth: 4 }],
    },
    {
      code: 'ifdef A\n    X = 1\nendif\n',
      filename: 'Makefile',
      options: [{ indentWidth: 4 }],
    },
  ],
  invalid: [
    {
      code: conditional,
      filename: path.join(root, 'width4', 'Makefile'),
      output: 'ifdef A\n    X = 1\nendif\n',
      errors: [{ messageId: 'unformatted' }],
    },
    {
      code: conditional,
      filename: path.join(root, 'width4', 'nested', 'width8', 'Makefile'),
      options: [{ indentWidth: 2 }],
      output: 'ifdef A\n        X = 1\nendif\n',
      errors: [{ messageId: 'unformatted' }],
    },
    {
      code: conditional,
      filename: path.join(root, 'empty', 'Makefile'),
      options: [{ indentConditionals: false }],
      output: 'ifdef A\n  X = 1\nendif\n',
      errors: [{ messageId: 'unformatted' }],
    },
    {
      code: 'A  = 1\nBB = 2\n',
      filename: path.join(root, 'no-align', 'Makefile'),
      output: 'A = 1\nBB = 2\n',
      errors: [{ messageId: 'unformatted' }],
    },
    {
      code: conditional,
      filename: path.join(root, 'none', 'Makefile'),
      options: [{ indentWidth: 4 }],
      output: 'ifdef A\n    X = 1\nendif\n',
      errors: [{ messageId: 'unformatted' }],
    },
    {
      code: 'X = 1\n',
      filename: path.join(root, 'invalid', 'Makefile'),
      output: null,
      errors: [
        {
          message:
            'makefmt: TOML parse error at line 1, column 16\n  |\n1 | indent_width = "four"\n  |                ^^^^^^\ninvalid type: string "four", expected usize\n',
          line: 1,
          column: 1,
        },
      ],
    },
    {
      code: 'X = 1\n',
      filename: path.join(root, 'unknown', 'sub', 'Makefile'),
      output: null,
      errors: [
        {
          message:
            'makefmt: TOML parse error at line 1, column 1\n  |\n1 | tab_width = 4\n  | ^^^^^^^^^\nunknown field `tab_width`, expected one of `align_assignments`, `align_continuations`, `indent_conditionals`, `indent_width`, `trim_trailing_whitespace`\n',
        },
      ],
    },
    {
      code: 'X=1\n',
      filename: path.join(root, 'broken', 'Makefile'),
      options: [{ indentWidth: 4 }],
      output: null,
      errors: [{ messageId: 'formatError' }],
    },
  ],
})

test('a .makefmt.toml that is a directory is reported', () => {
  mkdirSync(path.join(root, 'directory', '.makefmt.toml'), { recursive: true })
  const linter = new Linter({ cwd: root })
  const messages = linter.verify(
    'X = 1\n',
    {
      files: ['**/Makefile'],
      plugins: { makefmt },
      language: 'makefmt/makefile',
      rules: { 'makefmt/format': 'error' },
    },
    path.join(root, 'directory', 'Makefile')
  )
  assert.equal(messages.length, 1)
  assert.equal(messages[0].messageId, 'formatError')
  assert.match(messages[0].message, /^makefmt: EISDIR/)
})

test('plugin shape', () => {
  const packageJson = JSON.parse(
    readFileSync(new URL('../package.json', import.meta.url), 'utf8')
  )
  assert.deepEqual(makefmt.meta, {
    name: 'eslint-plugin-makefmt',
    version: packageJson.version,
  })
  assert.deepEqual(Object.keys(makefmt.languages), ['makefile', 'justfile'])
  assert.deepEqual(Object.keys(makefmt.rules), ['format'])
  assert.equal(makefmt.rules.format.meta?.fixable, 'code')
  assert.equal(makefmt.rules.format.meta?.type, 'layout')
  assert.deepEqual(
    makefmt.configs.recommended.map((config) => config.name),
    ['makefmt/makefile', 'makefmt/justfile']
  )
  for (const config of makefmt.configs.recommended) {
    assert.equal(config.plugins?.makefmt, makefmt)
  }
})

test('the rule ignores files of other languages', () => {
  const linter = new Linter()
  const messages = linter.verify(
    'const x   =   1\n',
    {
      plugins: { makefmt },
      rules: { 'makefmt/format': 'error' },
    },
    'index.js'
  )
  assert.deepEqual(messages, [])
})

test('the languages accept and ignore language options', () => {
  const linter = new Linter()
  const messages = linter.verify(
    'X=1\n',
    {
      files: ['**/Makefile'],
      plugins: { makefmt },
      language: 'makefmt/makefile',
      languageOptions: { globals: { foo: 'readonly' } },
      rules: { 'makefmt/format': 'error' },
    },
    'Makefile'
  )
  assert.equal(messages.length, 1)
  assert.equal(messages[0].messageId, 'unformatted')
})
