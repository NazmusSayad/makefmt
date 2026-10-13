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
import { ESLint } from 'eslint'
import makefmt from 'eslint-plugin-makefmt'

const makefile = 'CC=gcc\nCFLAGS   :=-O2\n\nifdef DEBUG\nCFLAGS+=-g\nendif\n'
const makefileFormatted =
  'CC     = gcc\nCFLAGS := -O2\n\nifdef DEBUG\n  CFLAGS += -g\nendif\n'
const makefileWidth4 =
  'CC     = gcc\nCFLAGS := -O2\n\nifdef DEBUG\n    CFLAGS += -g\nendif\n'
const justfile = 'x:="hello"\n\nbuild   target:\n  echo {{target}} {{x}}\n'
const justfileFormatted =
  'x := "hello"\n\nbuild target:\n    echo {{ target }} {{ x }}\n'

const makefilePaths = [
  'Makefile',
  'lower/makefile',
  'GNUmakefile',
  'rules.mk',
  'rules.mak',
  'rules.make',
  '.hidden.mk',
  'src/Makefile',
  'src/deep/dir/makefile',
  'src/deep/GNUmakefile',
  'src/lib/build.mk',
  '.config/tools.mak',
  'nested.dir/x.make',
]

const justfilePaths = [
  'justfile',
  'upper/Justfile',
  '.justfile',
  'tasks.just',
  '.hidden.just',
  'src/justfile',
  'src/deep/Justfile',
  'src/deep/dir/.justfile',
  '.config/release.just',
]

const ignoredPaths = [
  'Makefile.txt',
  'Makefile.bak',
  'notmakefile',
  'rules.mkd',
  'rules.mk.orig',
  'justfile.txt',
  'myjustfile',
  'tasks.justfile',
  'README.md',
]

const root = mkdtempSync(path.join(os.tmpdir(), 'eslint-plugin-makefmt-'))
after(() => rmSync(root, { recursive: true, force: true }))

function writeFile(relativePath: string, text: string) {
  const filePath = path.join(root, relativePath)
  mkdirSync(path.dirname(filePath), { recursive: true })
  writeFileSync(filePath, text)
}

for (const filePath of makefilePaths) writeFile(filePath, makefile)
for (const filePath of justfilePaths) writeFile(filePath, justfile)
for (const filePath of ignoredPaths) writeFile(filePath, 'X=1\n')
writeFile('configured/.makefmt.toml', 'indent_width = 4\n')
writeFile('configured/Makefile', makefile)
writeFile('configured/sub/rules.mk', makefile)
writeFile('configured/justfile', justfile)

const eslint = new ESLint({
  cwd: root,
  overrideConfigFile: true,
  overrideConfig: makefmt.configs.recommended,
})

const results = await eslint.lintFiles(['.'])

function resultFor(relativePath: string) {
  const result = results.find(
    (candidate) => candidate.filePath === path.join(root, relativePath)
  )
  if (result === undefined) throw new Error(`${relativePath} was not linted`)
  return result
}

test('lints exactly the Makefiles and justfiles', () => {
  const linted = results
    .map((result) => path.relative(root, result.filePath))
    .sort()
  const expected = [
    ...makefilePaths,
    ...justfilePaths,
    'configured/Makefile',
    'configured/sub/rules.mk',
    'configured/justfile',
  ]
    .map((relativePath) => path.normalize(relativePath))
    .sort()
  assert.deepEqual(linted, expected)
})

describe('Makefile patterns', () => {
  for (const relativePath of makefilePaths) {
    test(relativePath, async () => {
      const result = resultFor(relativePath)
      assert.equal(result.messages.length, 1)
      assert.equal(result.messages[0].ruleId, 'makefmt/format')
      assert.equal(result.messages[0].severity, 2)
      assert.equal(result.messages[0].message, 'File is not formatted')
      const config = await eslint.calculateConfigForFile(
        path.join(root, relativePath)
      )
      assert.equal(config.language, makefmt.languages.makefile)
    })
  }
})

describe('justfile patterns', () => {
  for (const relativePath of justfilePaths) {
    test(`${relativePath} uses the justfile language`, async () => {
      const result = resultFor(relativePath)
      assert.equal(result.messages.length, 1)
      assert.equal(result.messages[0].ruleId, 'makefmt/format')
      const config = await eslint.calculateConfigForFile(
        path.join(root, relativePath)
      )
      assert.equal(config.language, makefmt.languages.justfile)
    })
    test(`${relativePath} is reported as unformatted`, () => {
      const result = resultFor(relativePath)
      assert.equal(result.messages[0].message, 'File is not formatted')
    })
  }
})

describe('fix', () => {
  const fixer = new ESLint({
    cwd: root,
    overrideConfigFile: true,
    overrideConfig: makefmt.configs.recommended,
    fix: true,
  })

  for (const relativePath of makefilePaths) {
    test(relativePath, async () => {
      const fixed = await fixer.lintFiles([relativePath])
      assert.equal(fixed[0].output, makefileFormatted)
      assert.deepEqual(fixed[0].messages, [])
    })
  }

  for (const relativePath of justfilePaths) {
    test(relativePath, async () => {
      const fixed = await fixer.lintFiles([relativePath])
      assert.equal(fixed[0].output, justfileFormatted)
      assert.deepEqual(fixed[0].messages, [])
    })
  }

  test('uses the nearest .makefmt.toml', async () => {
    const fixed = await fixer.lintFiles([
      'configured/Makefile',
      'configured/sub/rules.mk',
    ])
    assert.equal(fixed.length, 2)
    for (const result of fixed) {
      assert.equal(result.output, makefileWidth4)
    }
  })

  test('ignores .makefmt.toml for justfiles', async () => {
    const fixed = await fixer.lintFiles(['configured/justfile'])
    assert.equal(fixed[0].output, justfileFormatted)
  })

  test('writes fixes to disk with outputFixes', async () => {
    writeFile('write/Makefile', makefile)
    const fixed = await fixer.lintFiles(['write/Makefile'])
    await ESLint.outputFixes(fixed)
    assert.equal(
      readFileSync(path.join(root, 'write', 'Makefile'), 'utf8'),
      makefileFormatted
    )
  })
})

test('formatted files have no problems', async () => {
  writeFile('clean/Makefile', makefileFormatted)
  writeFile('clean/rules.mk', makefileFormatted)
  const clean = await eslint.lintFiles(['clean'])
  assert.equal(clean.length, 2)
  for (const result of clean) assert.deepEqual(result.messages, [])
})

test('formatted justfiles have no problems', async () => {
  writeFile('clean-just/justfile', justfileFormatted)
  writeFile('clean-just/.justfile', justfileFormatted)
  const clean = await eslint.lintFiles(['clean-just'])
  assert.equal(clean.length, 2)
  for (const result of clean) assert.deepEqual(result.messages, [])
})

test('rule options from a custom config apply to Makefiles', async () => {
  writeFile('custom/Makefile', makefile)
  const custom = new ESLint({
    cwd: root,
    overrideConfigFile: true,
    overrideConfig: [
      ...makefmt.configs.recommended,
      {
        files: ['custom/**'],
        rules: { 'makefmt/format': ['warn', { indentWidth: 4 }] },
      },
    ],
    fix: true,
  })
  const fixed = await custom.lintFiles(['custom/Makefile'])
  assert.equal(fixed[0].output, makefileWidth4)
})

test('a custom setup without the recommended config', async () => {
  writeFile('manual/build.makefile', makefile)
  const manual = new ESLint({
    cwd: root,
    overrideConfigFile: true,
    overrideConfig: [
      {
        files: ['**/*.makefile'],
        plugins: { makefmt },
        language: 'makefmt/makefile',
        rules: {
          'makefmt/format': ['error', { alignAssignments: false }],
        },
      },
    ],
  })
  const manualResults = await manual.lintFiles(['manual'])
  assert.equal(manualResults.length, 1)
  assert.equal(manualResults[0].messages.length, 1)
  assert.equal(manualResults[0].messages[0].messageId, 'unformatted')
  const fixed = await new ESLint({
    cwd: root,
    overrideConfigFile: true,
    overrideConfig: [
      {
        files: ['**/*.makefile'],
        plugins: { makefmt },
        language: 'makefmt/makefile',
        rules: {
          'makefmt/format': ['error', { alignAssignments: false }],
        },
      },
    ],
    fix: true,
  }).lintFiles(['manual'])
  assert.equal(
    fixed[0].output,
    'CC = gcc\nCFLAGS := -O2\n\nifdef DEBUG\n  CFLAGS += -g\nendif\n'
  )
})

test('works next to JavaScript files', async () => {
  writeFile('mixed/index.js', 'const x   =   1\n')
  writeFile('mixed/Makefile', makefileFormatted)
  const mixed = new ESLint({
    cwd: root,
    overrideConfigFile: true,
    overrideConfig: [
      ...makefmt.configs.recommended,
      { files: ['**/*.js'], rules: { 'no-unused-vars': 'error' } },
    ],
  })
  const mixedResults = await mixed.lintFiles(['mixed'])
  assert.equal(mixedResults.length, 2)
  const javascript = mixedResults.find((result) =>
    result.filePath.endsWith('index.js')
  )
  const make = mixedResults.find((result) =>
    result.filePath.endsWith('Makefile')
  )
  assert.equal(javascript?.messages[0].ruleId, 'no-unused-vars')
  assert.deepEqual(make?.messages, [])
})
