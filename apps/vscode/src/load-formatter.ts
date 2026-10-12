interface FormatterExports {
  memory: WebAssembly.Memory
  alloc(len: number): number
  dealloc(ptr: number, len: number): void
  format(
    sourcePtr: number,
    sourceLen: number,
    configPtr: number,
    configLen: number
  ): number
  output_ptr(): number
  output_len(): number
}

export async function loadFormatter(wasm: Uint8Array) {
  const result = await WebAssembly.instantiate(new Uint8Array(wasm))
  const exports = result.instance.exports as unknown as FormatterExports
  const encoder = new TextEncoder()
  const decoder = new TextDecoder()

  function write(bytes: Uint8Array) {
    const ptr = exports.alloc(bytes.length) >>> 0
    new Uint8Array(exports.memory.buffer, ptr, bytes.length).set(bytes)
    return ptr
  }

  return function format(source: string, config: string) {
    const sourceBytes = encoder.encode(source)
    const configBytes = encoder.encode(config)
    const sourcePtr = write(sourceBytes)
    const configPtr = write(configBytes)
    const status = exports.format(
      sourcePtr,
      sourceBytes.length,
      configPtr,
      configBytes.length
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
    if (status === 0) return output
    if (status === 1) throw new Error(output)
    throw new Error(`Unexpected format status ${status}`)
  }
}
