//! The allocator. On macOS tern uses mimalloc and returns freed memory to the OS at once.
//!
//! A vault unlock runs scrypt at cost 2^18, which needs 256 MiB of scratch. libmalloc keeps that
//! freed block resident, and mimalloc's default 10 ms purge delay leaves it counted in the
//! footprint until the kernel reclaims it (349 MB at launch). With the delay at 0 it is 91 MB.
#![allow(unsafe_code)]

#[cfg(target_os = "macos")]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// `mi_option_purge_delay` in mimalloc 2.1+ (libmimalloc-sys 0.1.49 has no constant for it).
#[cfg(target_os = "macos")]
const PURGE_DELAY: libmimalloc_sys::mi_option_t = 15;

/// Makes mimalloc purge freed pages immediately. Call before the first large allocation.
pub fn init() {
    #[cfg(target_os = "macos")]
    // SAFETY: `mi_option_set` only stores an integer in mimalloc's option table, and any thread
    // may call it at any time.
    unsafe {
        libmimalloc_sys::mi_option_set(PURGE_DELAY, 0);
    }
}
