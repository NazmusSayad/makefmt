import * as vscode from 'vscode'
import { loadFormatter } from './load-formatter'

export async function activate(context: vscode.ExtensionContext) {
  const wasm = await vscode.workspace.fs.readFile(
    vscode.Uri.joinPath(context.extensionUri, 'dist', 'makefmt.wasm')
  )
  const format = await loadFormatter(wasm)

  context.subscriptions.push(
    vscode.languages.registerDocumentFormattingEditProvider('makefile', {
      async provideDocumentFormattingEdits(document) {
        try {
          const config = await readConfig(document)
          const text = document.getText()
          const formatted = format(text, config)
          if (formatted === text) return []
          const range = new vscode.Range(
            document.positionAt(0),
            document.positionAt(text.length)
          )
          return [vscode.TextEdit.replace(range, formatted)]
        } catch (error) {
          if (!(error instanceof Error)) throw error
          void vscode.window.showErrorMessage(`makefmt: ${error.message}`)
          return []
        }
      },
    })
  )
}

async function readConfig(document: vscode.TextDocument) {
  if (!document.isUntitled) {
    const projectConfig = await findProjectConfig(
      vscode.Uri.joinPath(document.uri, '..')
    )
    if (projectConfig !== undefined) return projectConfig
  }
  const settings = vscode.workspace.getConfiguration('makefmt', document.uri)
  return [
    `align_assignments = ${JSON.stringify(settings.get('alignAssignments'))}`,
    `align_continuations = ${JSON.stringify(settings.get('alignContinuations'))}`,
    `indent_conditionals = ${JSON.stringify(settings.get('indentConditionals'))}`,
    `indent_width = ${JSON.stringify(settings.get('indentWidth'))}`,
  ].join('\n')
}

async function findProjectConfig(
  directory: vscode.Uri
): Promise<string | undefined> {
  try {
    const bytes = await vscode.workspace.fs.readFile(
      vscode.Uri.joinPath(directory, '.makefmt.toml')
    )
    return new TextDecoder().decode(bytes)
  } catch (error) {
    if (
      !(error instanceof vscode.FileSystemError) ||
      error.code !== 'FileNotFound'
    ) {
      throw error
    }
  }
  const parent = vscode.Uri.joinPath(directory, '..')
  if (parent.path === directory.path) return undefined
  return findProjectConfig(parent)
}
