//! BLAKE3 helpers for configuration and artifact identity.

/// Hashes the supplied configuration bytes. Callers choose the effective
/// configuration or compatibility subset appropriate to their artifact.
pub fn config_hash(raw_toml_bytes: &[u8]) -> [u8; 32] {
    *blake3::hash(raw_toml_bytes).as_bytes()
}

/// Lowercase hex encoding of a config hash, for display in error messages
/// and logs. Round-trips through `blake3::Hash` rather than a hand-rolled
/// hex encoder (deliberately avoiding a `hex` crate dependency).
pub fn config_hash_hex(hash: &[u8; 32]) -> String {
    blake3::Hash::from(*hash).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_sensitive_to_bytes() {
        let a = config_hash(b"hello");
        let b = config_hash(b"hello");
        let c = config_hash(b"hellp");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn hex_round_trips_through_blake3() {
        let h = config_hash(b"config contents");
        let hex = config_hash_hex(&h);
        assert_eq!(hex.len(), 64);
        assert_eq!(blake3::Hash::from_hex(&hex).unwrap().as_bytes(), &h);
    }
}
