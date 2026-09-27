// CLOCK_MONOTONIC matches perf record --clockid mono; never use stdout arrival.
fn monotonic_ns() -> u64 {
    const CLOCK_MONOTONIC: std::ffi::c_int = 1;
    let mut value = ProcessCpuTimespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: the existing Linux GNU LP64 guard and layout assertions apply;
    // value is a live, aligned, exclusively borrowed writable stack object.
    let result = unsafe { clock_gettime(CLOCK_MONOTONIC, &mut value) };
    assert_eq!(
        result,
        0,
        "monotonic clock_gettime failed: {}",
        std::io::Error::last_os_error()
    );
    assert!(value.tv_sec >= 0 && (0..1_000_000_000).contains(&value.tv_nsec));
    (value.tv_sec as u64)
        .checked_mul(1_000_000_000)
        .and_then(|n| n.checked_add(value.tv_nsec as u64))
        .expect("monotonic nanoseconds overflow")
}

fn write_phases(
    path: &Path,
    case: &str,
    threads: usize,
    iterations: u64,
    bounds: [[u64; 2]; 8],
) -> std::io::Result<()> {
    // Order: CFR, state write, whole quality, EV0, EV1, BR0, BR1, gains.
    // The quality container overlaps its five explicitly ordered subintervals.
    for [start, end] in bounds {
        assert!(start < end, "empty or backwards phase interval");
    }
    assert!(bounds[0][1] <= bounds[1][0]);
    assert!(bounds[1][1] <= bounds[2][0]);
    assert!(bounds[2][0] <= bounds[3][0]);
    for pair in bounds[3..].windows(2) {
        assert!(pair[0][1] <= pair[1][0]);
    }
    assert!(bounds[7][1] <= bounds[2][1]);
    let names = [
        "cfr",
        "state_write",
        "quality",
        "ev_p0",
        "ev_p1",
        "br_p0",
        "br_p1",
        "exploitability",
    ];
    let phases = names
        .into_iter()
        .zip(bounds)
        .map(|(name, [start_ns, end_ns])| {
            format!("\"{name}\":{{\"start_ns\":{start_ns},\"end_ns\":{end_ns}}}")
        })
        .collect::<Vec<_>>()
        .join(",");
    let pid = std::process::id();
    write_json(
        path,
        &format!(
            "{{\"schema\":\"r1.flop-cpu-profile-phases/v1\",\"case\":\"{case}\",\"threads\":{threads},\"iterations\":{iterations},\"pid\":{pid},\"clock\":\"CLOCK_MONOTONIC\",\"clock_id\":1,\"unit\":\"nanoseconds\",\"interval\":\"[start_ns,end_ns)\",\"status\":\"completed\",\"phases\":{{{phases}}},\"performance_claim\":false}}"
        ),
    )
}

