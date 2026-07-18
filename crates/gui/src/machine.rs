//! Host machine facts feeding Auto mode's derivation
//! (`model::apply_auto_derivation`): detected logical core count and total
//! system RAM. Probed on demand, kept separate from `model` so the
//! derivation itself stays a pure function of already-known numbers (see
//! the Auto-mode phase B deliverable).

/// Logical core count Auto mode uses as its thread budget.
pub fn detected_threads() -> usize {
    std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1)
}

/// Half of total system RAM, in bytes -- Auto mode's dense-arena memory
/// budget. Only memory is probed (see `Cargo.toml`'s minimal `sysinfo`
/// feature selection: `default-features = false, features = ["system"]`).
pub fn half_of_total_memory_bytes() -> u64 {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    system.total_memory() / 2
}
