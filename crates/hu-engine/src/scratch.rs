//! LIFO scratch-buffer pool for the CFR and value walks.
//!
//! The walk acquires and releases `f32` buffers in strict recursion (stack)
//! order: a node takes its own buffers, recurses into children (which take
//! and release their own), then releases its buffers before returning to its
//! parent. A LIFO free-list therefore always hands back a buffer whose
//! capacity was last used at a nearby recursion depth. Buffers grow on demand
//! and are reused across iterations. Parallel walks lease an independent pool
//! from thread-local storage, including when Rayon reenters the same worker.

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

    /// As [`Self::take`], but reused elements are not zeroed: the caller
    /// must write every element before reading any. Only growth beyond the
    /// buffer's previous length is zero-filled. Debug builds fill the whole
    /// buffer with NaN instead, so a missed write shows up in tests.
    pub fn take_overwrite(&mut self, len: usize) -> Vec<f32> {
        let mut buf = self.free.pop().unwrap_or_default();
        if cfg!(debug_assertions) {
            buf.clear();
            buf.resize(len, f32::NAN);
        } else if buf.len() >= len {
            buf.truncate(len);
        } else {
            buf.resize(len, 0.0);
        }
        buf
    }

    /// Returns a buffer to the pool for reuse.
    pub fn put(&mut self, buf: Vec<f32>) {
        self.free.push(buf);
    }
}

// Take ownership before invoking Rayon: a worker can run a nested task while
// its outer task is suspended. Do not hold a RefCell borrow across `f`.
thread_local! {
    static WORKER_SCRATCH: std::cell::RefCell<Vec<Scratch>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

pub(crate) fn with_worker_scratch<R>(f: impl FnOnce(&mut Scratch) -> R) -> R {
    struct Lease(Scratch);
    impl Drop for Lease {
        fn drop(&mut self) {
            WORKER_SCRATCH.with(|pool| pool.borrow_mut().push(std::mem::take(&mut self.0)));
        }
    }
    let mut lease = Lease(WORKER_SCRATCH.with(|pool| pool.borrow_mut().pop().unwrap_or_default()));
    f(&mut lease.0)
}
