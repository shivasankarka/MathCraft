//! The C interface of the module. JavaScript copies the source into memory from `mc_alloc`, calls `mc_run`, reads the
//! JSON answer at `mc_output` for the length `mc_run` returned, and frees the source with `mc_free`. The clock comes
//! from the page: it must provide the import `mc_now_ms` (for example `performance.now`), and call `mc_init` once.
//!
//! This is the only `unsafe` in the project: raw pointers cross the boundary, and the page owns the other side.
#![allow(unsafe_code)]

use crate::Session;
use std::cell::RefCell;

thread_local! {
    static SESSION: RefCell<Session> = RefCell::new(Session::new());
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[link(wasm_import_module = "env")]
unsafe extern "C" {
    /// Milliseconds on a monotonic clock, from the page.
    fn mc_now_ms() -> f64;
}

fn host_clock() -> f64 {
    // SAFETY: the page provides `mc_now_ms`, which takes nothing and returns a number.
    unsafe { mc_now_ms() }
}

/// Install the page's clock; call once after instantiating the module.
#[unsafe(no_mangle)]
pub extern "C" fn mc_init() {
    mathcraft_core::clock::set_host_clock(host_clock);
}

/// `len` bytes of memory for the page to write the source into.
#[unsafe(no_mangle)]
pub extern "C" fn mc_alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// Give back memory from `mc_alloc`.
///
/// # Safety
/// `ptr` and `len` must be exactly what `mc_alloc` returned and was asked for, and not freed before.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mc_free(ptr: *mut u8, len: usize) {
    // SAFETY: by the contract above, this is a `Vec<u8>` of capacity `len` that `mc_alloc` leaked.
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len) });
}

/// Run the UTF-8 source at `ptr` (`len` bytes) in the session with a limit of `timeout_ms`; the JSON answer is left at
/// `mc_output` and its length is returned.
///
/// # Safety
/// `ptr` must point at `len` readable bytes, as filled in by the page after `mc_alloc`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mc_run(ptr: *const u8, len: usize, timeout_ms: u32) -> usize {
    // SAFETY: by the contract above.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let answer = match std::str::from_utf8(bytes) {
        Ok(code) => SESSION.with(|s| s.borrow_mut().run(code, timeout_ms)),
        Err(_) => r#"{"stdout":"","value":null,"latex":null,"error":"the source is not valid UTF-8"}"#.to_owned(),
    };
    OUTPUT.with(|o| {
        let mut o = o.borrow_mut();
        *o = answer.into_bytes();
        o.len()
    })
}

/// Where the last answer from `mc_run` is.
#[unsafe(no_mangle)]
pub extern "C" fn mc_output() -> *const u8 {
    OUTPUT.with(|o| o.borrow().as_ptr())
}

/// Forget every variable of the session.
#[unsafe(no_mangle)]
pub extern "C" fn mc_reset() {
    SESSION.with(|s| s.borrow_mut().reset());
}
