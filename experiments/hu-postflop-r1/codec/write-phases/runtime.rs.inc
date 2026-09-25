// Research-only: appended to an exact source copy by apply.py.
mod r1_write_phase {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::fs::File;
    use std::io::{self, IoSlice, Seek, SeekFrom, Write};
    use std::time::Instant;

    const NAMES: [&str; 13] = [
        "validation", "size_and_group", "metadata_prepare", "serialize",
        "metadata_compress", "temp_create", "pair_refs", "hash",
        "chunk_compress_excluding_file", "file_write", "file_seek", "sync", "persist",
    ];

    #[derive(Default, serde::Serialize)]
    pub struct Leaf { pub ns: u128, pub calls: u64 }

    #[derive(serde::Serialize)]
    pub struct Record {
        pub schema: &'static str,
        pub outcome: &'static str,
        pub inner_total_ns: u128,
        pub leaves: BTreeMap<&'static str, Leaf>,
        pub unclassified_ns: Option<u128>,
        pub compression_envelope_ns: u128,
        pub compression_nested_file_ns: u128,
        pub compression_groups: u64,
        pub compression_written_bytes: u64,
        pub compression_expected_bytes: u64,
        pub chunk_byte_counts_valid: bool,
        pub successful_write_bytes: u64,
        pub file_write_calls: u64,
        pub file_flush_calls: u64,
        pub file_errors: u64,
        pub subtraction_valid: bool,
    }

    pub struct Clock { started: Instant, data: RefCell<Record>, last_chunk_bytes: RefCell<u64> }

    impl Clock {
        pub fn new() -> Self {
            let data = Record {
                schema: "r1.sol-write-phases/v1", outcome: "incomplete",
                inner_total_ns: 0,
                leaves: NAMES.into_iter().map(|name| (name, Leaf::default())).collect(),
                unclassified_ns: None, compression_envelope_ns: 0,
                compression_nested_file_ns: 0, compression_groups: 0,
                compression_written_bytes: 0, file_write_calls: 0,
                compression_expected_bytes: 0, chunk_byte_counts_valid: true,
                successful_write_bytes: 0,
                file_flush_calls: 0, file_errors: 0, subtraction_valid: true,
            };
            Self { started: Instant::now(), data: RefCell::new(data), last_chunk_bytes: RefCell::new(0) }
        }

        pub fn measure<T>(&self, name: &'static str, f: impl FnOnce() -> T) -> T {
            let start = Instant::now();
            let value = f();
            let elapsed = start.elapsed().as_nanos();
            let mut data = self.data.borrow_mut();
            let leaf = data.leaves.get_mut(name).expect("fixed phase name");
            leaf.ns += elapsed;
            leaf.calls += 1;
            value
        }

        pub fn compress<T>(&self, f: impl FnOnce() -> T) -> T {
            let before = self.data.borrow().leaves["file_write"].ns;
            let bytes_before = self.data.borrow().successful_write_bytes;
            let start = Instant::now();
            let value = f();
            let elapsed = start.elapsed().as_nanos();
            let mut data = self.data.borrow_mut();
            let nested = data.leaves["file_write"].ns - before;
            data.compression_envelope_ns += elapsed;
            data.compression_nested_file_ns += nested;
            data.compression_groups += 1;
            let written = data.successful_write_bytes - bytes_before;
            data.compression_written_bytes += written;
            *self.last_chunk_bytes.borrow_mut() = written;
            match elapsed.checked_sub(nested) {
                Some(exclusive) => {
                    let leaf = data.leaves.get_mut("chunk_compress_excluding_file").unwrap();
                    leaf.ns += exclusive;
                    leaf.calls += 1;
                }
                None => data.subtraction_valid = false,
            }
            value
        }

        pub fn check_chunk_bytes(&self, expected: u64) {
            let mut data = self.data.borrow_mut();
            data.compression_expected_bytes += expected;
            data.chunk_byte_counts_valid &= expected == *self.last_chunk_bytes.borrow();
        }

        pub fn finish(self, success: bool) -> Record {
            let total = self.started.elapsed().as_nanos();
            let mut data = self.data.into_inner();
            data.outcome = if success { "completed" } else { "error" };
            data.inner_total_ns = total;
            data.unclassified_ns = total.checked_sub(data.leaves.values().map(|v| v.ns).sum());
            data
        }
    }

    pub struct FileTap<'a> { pub inner: &'a mut File, pub clock: &'a Clock }
    impl FileTap<'_> {
        pub fn sync_all(&self) -> io::Result<()> {
            self.clock.measure("sync", || self.inner.sync_all())
        }
        fn write_result<T>(&self, result: &io::Result<T>) {
            let mut data = self.clock.data.borrow_mut();
            data.file_write_calls += 1;
            if result.is_err() { data.file_errors += 1; }
        }
    }
    impl Write for FileTap<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let result = self.clock.measure("file_write", || self.inner.write(bytes));
            self.write_result(&result);
            if let Ok(n) = &result { self.clock.data.borrow_mut().successful_write_bytes += *n as u64; }
            result
        }
        fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
            let result = self.clock.measure("file_write", || self.inner.write_all(bytes));
            self.write_result(&result);
            if result.is_ok() { self.clock.data.borrow_mut().successful_write_bytes += bytes.len() as u64; }
            result
        }
        fn write_vectored(&mut self, bytes: &[IoSlice<'_>]) -> io::Result<usize> {
            let result = self.clock.measure("file_write", || self.inner.write_vectored(bytes));
            self.write_result(&result);
            if let Ok(n) = &result { self.clock.data.borrow_mut().successful_write_bytes += *n as u64; }
            result
        }
        fn flush(&mut self) -> io::Result<()> {
            let result = self.clock.measure("file_write", || self.inner.flush());
            let mut data = self.clock.data.borrow_mut();
            data.file_flush_calls += 1;
            if result.is_err() { data.file_errors += 1; }
            result
        }
    }
    impl Seek for FileTap<'_> {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.clock.measure("file_seek", || self.inner.seek(position))
        }
        fn stream_position(&mut self) -> io::Result<u64> {
            self.clock.measure("file_seek", || self.inner.stream_position())
        }
    }
}

pub use r1_write_phase::Record as R1WritePhaseRecord;

pub fn r1_write_sol_profiled(path: &Path, payload: &SolPayload) -> (Result<(), SolError>, R1WritePhaseRecord) {
    let phases = r1_write_phase::Clock::new();
    let result = r1_write_sol_measured(path, payload, &phases);
    let record = phases.finish(result.is_ok());
    (result, record)
}
