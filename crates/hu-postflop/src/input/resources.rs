use super::Storage;
use crate::MemoryEstimate;
use std::io;

/// Explicit bytes, or floor(80% of physical RAM). Pure and overflow-safe.
pub fn resolve_memory_limit(explicit_bytes: Option<u64>, physical_bytes: u64) -> u64 {
    explicit_bytes.unwrap_or_else(|| physical_bytes / 5 * 4 + physical_bytes % 5 * 4 / 5)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("resource limit: P1 memory estimate {required} bytes exceeds memory limit {limit} bytes")]
pub struct MemoryLimitError {
    pub required: u64,
    pub limit: u64,
}

/// Check storage plus packed-save and codec workspace before building a game.
/// The CLI maps this resource error to exit code 75, as for P2.
pub fn check_memory_limit(
    estimate: &MemoryEstimate,
    storage: Storage,
    limit: u64,
) -> Result<(), MemoryLimitError> {
    let required = match storage {
        Storage::F32 => estimate.f32_bytes,
        Storage::I16 => estimate.i16_bytes,
    }
    .saturating_add(estimate.save_bytes)
    .saturating_add(estimate.compression_bytes);
    if required > limit {
        Err(MemoryLimitError { required, limit })
    } else {
        Ok(())
    }
}

/// OS boundary for automatic memory limits. No fallback on query failure.
#[cfg(windows)]
pub fn physical_memory_bytes() -> io::Result<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: status is writable and its size is provided to the Windows API.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(status.ullTotalPhys)
    }
}

#[cfg(unix)]
pub fn physical_memory_bytes() -> io::Result<u64> {
    // SAFETY: sysconf takes named constants and no pointers.
    let pages = unsafe { libc::sysconf(libc::_SC_PHYS_PAGES) };
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if pages <= 0 || page_size <= 0 {
        return Err(io::Error::other(
            "could not query physical RAM with sysconf",
        ));
    }
    (pages as u64)
        .checked_mul(page_size as u64)
        .ok_or_else(|| io::Error::other("physical RAM size overflows u64"))
}

#[cfg(not(any(windows, unix)))]
pub fn physical_memory_bytes() -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "physical RAM query requires Windows or Unix",
    ))
}
