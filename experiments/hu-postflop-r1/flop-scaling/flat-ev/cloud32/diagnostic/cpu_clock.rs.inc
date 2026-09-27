// Linux x86_64 LP64 GNU ABI only; see diagnostic/README.jp.md for primary sources.
#[cfg(not(all(
    target_os = "linux",
    target_arch = "x86_64",
    target_env = "gnu",
    target_pointer_width = "64"
)))]
compile_error!("process CPU diagnostic requires x86_64-unknown-linux-gnu LP64");

#[repr(C)]
struct ProcessCpuTimespec {
    tv_sec: std::ffi::c_long,
    tv_nsec: std::ffi::c_long,
}

const _: () = {
    assert!(std::mem::size_of::<std::ffi::c_int>() == 4);
    assert!(std::mem::size_of::<std::ffi::c_long>() == 8);
    assert!(std::mem::size_of::<ProcessCpuTimespec>() == 16);
    assert!(std::mem::align_of::<ProcessCpuTimespec>() == 8);
    assert!(std::mem::offset_of!(ProcessCpuTimespec, tv_sec) == 0);
    assert!(std::mem::offset_of!(ProcessCpuTimespec, tv_nsec) == 8);
};

unsafe extern "C" {
    fn clock_gettime(clock_id: std::ffi::c_int, value: *mut ProcessCpuTimespec) -> std::ffi::c_int;
}

fn process_cpu_ns() -> u64 {
    const CLOCK_PROCESS_CPUTIME_ID: std::ffi::c_int = 2;
    let mut value = ProcessCpuTimespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: the guarded LP64 ABI matches clockid_t and struct timespec;
    // value is a live, aligned, exclusively borrowed writable stack object.
    let result = unsafe { clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &mut value) };
    assert_eq!(result, 0, "clock_gettime failed: {}", std::io::Error::last_os_error());
    assert!(value.tv_sec >= 0 && (0..1_000_000_000).contains(&value.tv_nsec));
    (value.tv_sec as u64)
        .checked_mul(1_000_000_000)
        .and_then(|n| n.checked_add(value.tv_nsec as u64))
        .expect("process CPU nanoseconds overflow")
}

fn process_cpu_elapsed(started: u64) -> f64 {
    process_cpu_ns()
        .checked_sub(started)
        .expect("process CPU clock moved backwards") as f64
        * 1e-9
}
