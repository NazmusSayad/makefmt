use std::cell::RefCell;

thread_local! {
    static OUTPUT: RefCell<String> = const { RefCell::new(String::new()) };
}

#[unsafe(no_mangle)]
extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

#[unsafe(no_mangle)]
unsafe extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    unsafe { drop(Vec::from_raw_parts(ptr, 0, len)) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn format(
    source_ptr: *const u8,
    source_len: usize,
    config_ptr: *const u8,
    config_len: usize,
) -> u32 {
    let source = unsafe { std::slice::from_raw_parts(source_ptr, source_len) };
    let config = unsafe { std::slice::from_raw_parts(config_ptr, config_len) };
    let (status, text) = match run(source, config) {
        Ok(text) => (0, text),
        Err(message) => (1, message),
    };
    OUTPUT.with_borrow_mut(|output| *output = text);
    status
}

#[unsafe(no_mangle)]
extern "C" fn output_ptr() -> *const u8 {
    OUTPUT.with_borrow(|output| output.as_ptr())
}

#[unsafe(no_mangle)]
extern "C" fn output_len() -> usize {
    OUTPUT.with_borrow(|output| output.len())
}

fn run(source: &[u8], config: &[u8]) -> Result<String, String> {
    let source =
        std::str::from_utf8(source).map_err(|error| format!("Invalid UTF-8 in source: {error}"))?;
    let config =
        std::str::from_utf8(config).map_err(|error| format!("Invalid UTF-8 in config: {error}"))?;
    let options = makefmt::Options::from_toml(config)?;
    Ok(makefmt::format(source, &options))
}
