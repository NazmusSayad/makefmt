interface FormatterExports {
  memory: WebAssembly.Memory
  alloc(len: number): number
  dealloc(ptr: number, len: number): void
  format(
    sourcePtr: number,
    sourceLen: number,
    configPtr: number,
    configLen: number,
    language: number
  ): number
  output_ptr(): number
  output_len(): number
}

export type Language = 'makefile' | 'justfile'

const languageCodes = { makefile: 0, justfile: 1 } as const

export function loadFormatter(wasm: Uint8Array) {
  const module = new WebAssembly.Module(new Uint8Array(wasm))
  const encoder = new TextEncoder()
  const decoder = new TextDecoder()

  function instantiate() {
    return new WebAssembly.Instance(module)
      .exports as unknown as FormatterExports
  }

  let exports = instantiate()

  function write(bytes: Uint8Array) {
    const ptr = exports.alloc(bytes.length) >>> 0
    new Uint8Array(exports.memory.buffer, ptr, bytes.length).set(bytes)
    return ptr
  }

  function run(source: string, config: string, language: Language) {
    const sourceBytes = encoder.encode(source)
    const configBytes = encoder.encode(config)
    const sourcePtr = write(sourceBytes)
    const configPtr = write(configBytes)
    const status = exports.format(
      sourcePtr,
      sourceBytes.length,
      configPtr,
      configBytes.length,
      languageCodes[language]
    )
    exports.dealloc(sourcePtr, sourceBytes.length)
    exports.dealloc(configPtr, configBytes.length)
    const output = decoder.decode(
      new Uint8Array(
        exports.memory.buffer,
        exports.output_ptr() >>> 0,
        exports.output_len() >>> 0
      )
    )
    return { status, output }
  }

  return function format(source: string, config: string, language: Language) {
    let result
    try {
      result = run(source, config, language)
    } catch (error) {
      exports = instantiate()
      throw error
    }
    if (result.status === 0) return result.output
    if (result.status === 1) throw new Error(result.output)
    throw new Error(`Unexpected format status ${result.status}`)
  }
}
