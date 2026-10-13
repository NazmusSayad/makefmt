import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { after, test } from 'node:test'
import { fileURLToPath } from 'node:url'

const oxlint = fileURLToPath(
  new URL('../node_modules/.bin/oxlint', import.meta.url)
)
const pluginPath = fileURLToPath(new URL('../dist/index.js', import.meta.url))

const makefile = 'CC=gcc\nX  :=  1\n'
const justfile = 'x:="hello"\n\nbuild:\n  echo {{x}}\n'

const root = mkdtempSync(path.join(os.tmpdir(), 'eslint-plugin-makefmt-'))
after(() => rmSync(root, { recursive: true, force: true }))

writeFileSync(path.join(root, 'Makefile'), makefile)
writeFileSync(path.join(root, 'rules.mk'), makefile)
writeFileSync(path.join(root, 'justfile'), justfile)
writeFileSync(path.join(root, 'index.js'), 'debugger\n')

function run(config: object, args: string[]) {
  writeFileSync(path.join(root, '.oxlintrc.json'), JSON.stringify(config))
  const result = spawnSync(oxlint, ['-c', '.oxlintrc.json', ...args], {
    cwd: root,
    encoding: 'utf8',
  })
  return { status: result.status, output: result.stdout + result.stderr }
}

test('oxlint loads the plugin as a JS plugin', () => {
  const result = run(
    {
      jsPlugins: [pluginPath],
      categories: { correctness: 'off' },
      rules: { 'no-debugger': 'error', 'makefmt/format': 'error' },
    },
    ['index.js']
  )
  assert.equal(result.status, 1)
  assert.match(result.output, /no-debugger/)
  assert.doesNotMatch(result.output, /makefmt/)
})

test('oxlint finds the rules of the plugin', () => {
  const result = run(
    { jsPlugins: [pluginPath], rules: { 'makefmt/missing': 'error' } },
    ['index.js']
  )
  assert.equal(result.status, 1)
  assert.match(result.output, /Rule 'missing' not found in plugin 'makefmt'/)
})

test('oxlint accepts valid rule options', () => {
  const result = run(
    {
      jsPlugins: [pluginPath],
      categories: { correctness: 'off' },
      rules: {
        'makefmt/format': [
          'error',
          {
            alignAssignments: false,
            alignContinuations: true,
            indentConditionals: false,
            indentWidth: 4,
          },
        ],
      },
    },
    ['index.js']
  )
  assert.equal(result.status, 0)
  assert.equal(result.output, '')
})

test('oxlint validates rule options against the schema', () => {
  const result = run(
    {
      jsPlugins: [pluginPath],
      rules: { 'makefmt/format': ['error', { indentWidth: -1 }] },
    },
    ['index.js']
  )
  assert.equal(result.status, 1)
  assert.match(
    result.output,
    /Options validation failed for rule 'makefmt\/format'/
  )
})

test('oxlint cannot lint Makefiles or justfiles passed explicitly', () => {
  const result = run(
    {
      jsPlugins: [pluginPath],
      categories: { correctness: 'off' },
      overrides: [
        {
          files: ['**/Makefile', '**/*.mk', '**/justfile'],
          rules: { 'makefmt/format': 'error' },
        },
      ],
    },
    ['Makefile', 'rules.mk', 'justfile']
  )
  assert.equal(result.status, 1)
  assert.match(result.output, /No files found to lint/)
})

test('oxlint skips Makefiles and justfiles in directories, even with --fix', () => {
  const result = run(
    {
      jsPlugins: [pluginPath],
      categories: { correctness: 'off' },
      overrides: [
        {
          files: ['**/Makefile', '**/*.mk', '**/justfile'],
          rules: { 'makefmt/format': 'error' },
        },
      ],
    },
    ['--fix', '.']
  )
  assert.equal(result.status, 0)
  assert.doesNotMatch(result.output, /makefmt/)
  assert.equal(readFileSync(path.join(root, 'Makefile'), 'utf8'), makefile)
  assert.equal(readFileSync(path.join(root, 'rules.mk'), 'utf8'), makefile)
  assert.equal(readFileSync(path.join(root, 'justfile'), 'utf8'), justfile)
})
