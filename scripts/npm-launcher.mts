#!/usr/bin/env node
import { spawnSync } from 'node:child_process'
import path from 'node:path'

const binaries: Record<string, string> = {
  'darwin-arm64': 'makefmt',
  'darwin-x64': 'makefmt',
  'linux-arm64': 'makefmt',
  'linux-x64': 'makefmt',
  'win32-x64': 'makefmt.exe',
}

const target = `${process.platform}-${process.arch}`
const binary = binaries[target]
if (!binary) {
  console.error(`makefmt does not support ${target}`)
  process.exit(2)
}

const result = spawnSync(
  path.join(import.meta.dirname, target, binary),
  process.argv.slice(2),
  { stdio: 'inherit' }
)
if (result.error) {
  console.error(`makefmt: ${result.error.message}`)
  process.exit(2)
}
if (result.signal) process.kill(process.pid, result.signal)
process.exit(result.status)
