import { readFileSync } from 'node:fs'
import path from 'node:path'
import type {
  File,
  Language,
  LanguageOptions,
  OkParseResult,
  RuleDefinition,
  RuleVisitor,
  SourceLocation,
} from '@eslint/core'
import { TextSourceCodeBase, VisitNodeStep } from '@eslint/plugin-kit'
import { loadFormatter } from '@makefmt/wasm'
import type { ESLint, Linter } from 'eslint'

interface RootNode {
  type: 'Makefile' | 'Justfile'
  range: [number, number]
  loc: SourceLocation
}

interface FormatOptions {
  alignAcrossComments?: boolean
  alignAssignments?: boolean
  alignContinuations?: boolean
  formatRecipes?: boolean
  indentConditionals?: boolean
  indentWidth?: number
  maxBlankLines?: number
  trimTrailingWhitespace?: boolean
}

const wasm = readFileSync(new URL('./makefmt.wasm', import.meta.url))
const format = loadFormatter(wasm)

const packageJson: { version: string } = JSON.parse(
  readFileSync(new URL('../package.json', import.meta.url), 'utf8')
)

class MakefmtSourceCode extends TextSourceCodeBase<{
  LangOptions: LanguageOptions
  RootNode: RootNode
  SyntaxElementWithLoc: RootNode
  ConfigNode: never
}> {
  getParent() {
    return undefined
  }

  *traverse() {
    yield new VisitNodeStep({
      target: this.ast,
      phase: 1,
      args: [this.ast, undefined],
    })
    yield new VisitNodeStep({
      target: this.ast,
      phase: 2,
      args: [this.ast, undefined],
    })
  }
}

class MakefmtLanguage implements Language<{
  LangOptions: LanguageOptions
  Code: MakefmtSourceCode
  RootNode: RootNode
  Node: RootNode
}> {
  fileType = 'text' as const
  lineStart = 1 as const
  columnStart = 0 as const
  nodeTypeKey = 'type'
  visitorKeys = { Makefile: [], Justfile: [] }
  #rootType: RootNode['type']

  constructor(rootType: RootNode['type']) {
    this.#rootType = rootType
  }

  validateLanguageOptions() {}

  parse(file: File) {
    const text = fileText(file)
    const lines = text.split(/\r?\n/u)
    return {
      ok: true as const,
      ast: {
        type: this.#rootType,
        range: [0, text.length] satisfies [number, number],
        loc: {
          start: { line: 1, column: 0 },
          end: { line: lines.length, column: lines[lines.length - 1].length },
        },
      },
    }
  }

  createSourceCode(file: File, input: OkParseResult<RootNode>) {
    return new MakefmtSourceCode({ text: fileText(file), ast: input.ast })
  }
}

function fileText(file: File) {
  if (typeof file.body !== 'string') {
    throw new TypeError(`Expected the text of ${file.path} as a string`)
  }
  return file.body
}

const formatRule: RuleDefinition<{
  LangOptions: LanguageOptions
  Code: MakefmtSourceCode
  RuleOptions: [FormatOptions?]
  Visitor: RuleVisitor
  Node: RootNode
  MessageIds: 'unformatted' | 'formatError'
  ExtRuleDocs: object
}> = {
  meta: {
    type: 'layout',
    docs: {
      description: 'Require Makefiles and justfiles to be formatted by makefmt',
      url: 'https://github.com/NazmusSayad/makefmt/tree/main/apps/eslint-plugin#readme',
    },
    fixable: 'code',
    schema: [
      {
        type: 'object',
        properties: {
          alignAcrossComments: { type: 'boolean' },
          alignAssignments: { type: 'boolean' },
          alignContinuations: { type: 'boolean' },
          formatRecipes: { type: 'boolean' },
          indentConditionals: { type: 'boolean' },
          indentWidth: { type: 'integer', minimum: 0 },
          maxBlankLines: { type: 'integer', minimum: 1 },
          trimTrailingWhitespace: { type: 'boolean' },
        },
        additionalProperties: false,
      },
    ],
    messages: {
      unformatted: 'File is not formatted',
      formatError: 'makefmt: {{message}}',
    },
  },

  create(context) {
    function check(language: 'makefile' | 'justfile') {
      const sourceCode = context.sourceCode
      const text = sourceCode.text
      let formatted
      try {
        const config =
          language === 'makefile'
            ? readConfig(context.physicalFilename, context.options[0])
            : ''
        formatted = format(text, config, language)
      } catch (error) {
        if (!(error instanceof Error)) throw error
        context.report({
          loc: { line: 1, column: 0 },
          messageId: 'formatError',
          data: { message: error.message },
        })
        return
      }
      if (formatted === text) return

      let start = 0
      while (
        start < text.length &&
        start < formatted.length &&
        text[start] === formatted[start]
      ) {
        start++
      }
      let end = 0
      while (
        end < text.length - start &&
        end < formatted.length - start &&
        text[text.length - 1 - end] === formatted[formatted.length - 1 - end]
      ) {
        end++
      }
      const replacement = formatted.slice(start, formatted.length - end)
      context.report({
        loc: {
          start: sourceCode.getLocFromIndex(start),
          end: sourceCode.getLocFromIndex(text.length - end),
        },
        messageId: 'unformatted',
        fix(fixer) {
          return fixer.replaceTextRange([start, text.length - end], replacement)
        },
      })
    }

    return {
      Makefile() {
        check('makefile')
      },
      Justfile() {
        check('justfile')
      },
    }
  },
}

function readConfig(filename: string, options: FormatOptions | undefined) {
  if (path.isAbsolute(filename)) {
    const projectConfig = findProjectConfig(path.dirname(filename))
    if (projectConfig !== undefined) return projectConfig
  }
  if (options === undefined) return ''
  const lines = []
  if (options.alignAcrossComments !== undefined) {
    lines.push(`align_across_comments = ${options.alignAcrossComments}`)
  }
  if (options.alignAssignments !== undefined) {
    lines.push(`align_assignments = ${options.alignAssignments}`)
  }
  if (options.alignContinuations !== undefined) {
    lines.push(`align_continuations = ${options.alignContinuations}`)
  }
  if (options.formatRecipes !== undefined) {
    lines.push(`format_recipes = ${options.formatRecipes}`)
  }
  if (options.indentConditionals !== undefined) {
    lines.push(`indent_conditionals = ${options.indentConditionals}`)
  }
  if (options.indentWidth !== undefined) {
    lines.push(`indent_width = ${options.indentWidth}`)
  }
  if (options.maxBlankLines !== undefined) {
    lines.push(`max_blank_lines = ${options.maxBlankLines}`)
  }
  if (options.trimTrailingWhitespace !== undefined) {
    lines.push(`trim_trailing_whitespace = ${options.trimTrailingWhitespace}`)
  }
  return lines.join('\n')
}

function findProjectConfig(directory: string): string | undefined {
  try {
    return readFileSync(path.join(directory, '.makefmt.toml'), 'utf8')
  } catch (error) {
    if (!(error instanceof Error) || !('code' in error)) throw error
    if (error.code !== 'ENOENT') throw error
  }
  const parent = path.dirname(directory)
  if (parent === directory) return undefined
  return findProjectConfig(parent)
}

const recommended: Linter.Config[] = []

const plugin: ESLint.Plugin & {
  languages: NonNullable<ESLint.Plugin['languages']>
  rules: NonNullable<ESLint.Plugin['rules']>
  configs: { recommended: Linter.Config[] }
} = {
  meta: { name: 'eslint-plugin-makefmt', version: packageJson.version },
  languages: {
    makefile: new MakefmtLanguage('Makefile'),
    justfile: new MakefmtLanguage('Justfile'),
  },
  rules: { format: formatRule },
  configs: { recommended },
}

recommended.push(
  {
    name: 'makefmt/makefile',
    files: [
      '**/Makefile',
      '**/makefile',
      '**/GNUmakefile',
      '**/*.mk',
      '**/*.mak',
      '**/*.make',
    ],
    plugins: { makefmt: plugin },
    language: 'makefmt/makefile',
    rules: { 'makefmt/format': 'error' },
  },
  {
    name: 'makefmt/justfile',
    files: ['**/justfile', '**/Justfile', '**/.justfile', '**/*.just'],
    plugins: { makefmt: plugin },
    language: 'makefmt/justfile',
    rules: { 'makefmt/format': 'error' },
  }
)

export default plugin
