import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { stripTypeScriptTypes } from 'node:module'
import path from 'node:path'

const root = path.join(import.meta.dirname, '..')
const output = path.join(root, 'dist', 'npm')

const manifest = {
  name: 'makefmt',
  version: '0.0.0',
  type: 'module',
  description: 'Fast, opinionated Makefile formatter',
  keywords: ['makefile', 'make', 'formatter', 'fmt'],
  license: 'MIT',
  repository: {
    type: 'git',
    url: 'git+https://github.com/NazmusSayad/makefmt.git',
  },
  bin: { makefmt: 'bin/makefmt.js' },
  engines: { node: '>=20.11' },
}

mkdirSync(path.join(output, 'bin'), { recursive: true })
writeFileSync(
  path.join(output, 'bin', 'makefmt.js'),
  stripTypeScriptTypes(
    readFileSync(path.join(import.meta.dirname, 'npm-launcher.mts'), 'utf8')
  )
)
copyFileSync(path.join(root, 'LICENSE'), path.join(output, 'LICENSE'))
copyFileSync(path.join(root, 'README.md'), path.join(output, 'README.md'))
writeFileSync(
  path.join(output, 'package.json'),
  `${JSON.stringify(manifest, null, 2)}\n`
)
console.log(`npm package written to ${output}`)
