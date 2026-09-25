//! LIFO scratch-buffer pool for the CFR and value walks.
//!
//! The walk acquires and releases `f32` buffers in strict recursion (stack)
//! order: a node takes its own buffers, recurses into children (which take
//! and release their own), then releases its buffers before returning to its
//! parent. A LIFO free-list therefore always hands back a buffer whose
//! capacity was last used at the same recursion depth for the same purpose,
//! so capacities stay matched across iterations and steady-state solving
//! allocates nothing.

/// LIFO free-list of `f32` buffers.
#[derive(Default)]
pub struct Scratch {
    free: Vec<Vec<f32>>,
}

impl Scratch {
    pub fn new() -> Self {
        Scratch { free: Vec::new() }
    }

    /// Pops a buffer (or allocates a new one), resizing it to `len` zeros.
    pub fn take(&mut self, len: usize) -> Vec<f32> {
        let mut buf = self.free.pop().unwrap_or_default();
        buf.clear();
        buf.resize(len, 0.0);
        buf
    }

    /// Returns a buffer to the pool for reuse.
    pub fn put(&mut self, buf: Vec<f32>) {
        self.free.push(buf);
    }
}
