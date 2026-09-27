// Research-only phase-window allocation counters. No solver semantics live here.
mod allocation_probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    // The event driver is single-threaded. An odd epoch admits operations; an
    // even epoch excludes them. Epoch equality prevents a delayed old entrant
    // from being admitted after a stop/reset/start cycle.
    static EPOCH: AtomicU64 = AtomicU64::new(0);
    static INFLIGHT: AtomicUsize = AtomicUsize::new(0);
    static COUNTERS: [AtomicU64; 12] = [const { AtomicU64::new(0) }; 12];

    pub(crate) struct CountingSystem;

    #[global_allocator]
    pub(crate) static ALLOCATOR: CountingSystem = CountingSystem;

    fn enter() -> bool {
        let epoch = EPOCH.load(Ordering::SeqCst);
        if epoch & 1 == 0 {
            return false;
        }
        INFLIGHT.fetch_add(1, Ordering::SeqCst);
        if EPOCH.load(Ordering::SeqCst) != epoch {
            INFLIGHT.fetch_sub(1, Ordering::SeqCst);
            return false;
        }
        true
    }

    fn add(index: usize, value: u64) {
        COUNTERS[index].fetch_add(value, Ordering::Relaxed);
    }

    fn leave() {
        INFLIGHT.fetch_sub(1, Ordering::SeqCst);
    }

    // Each admitted operation stays inflight through System and all updates.
    // Allocator methods never format, allocate recursively, lock, or panic.
    unsafe impl GlobalAlloc for CountingSystem {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let counted = enter();
            let result = unsafe { System.alloc(layout) };
            if counted {
                add(0, 1);
                add(1, layout.size() as u64);
                add(2, result.is_null() as u64);
                leave();
            }
            result
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let counted = enter();
            let result = unsafe { System.alloc_zeroed(layout) };
            if counted {
                add(3, 1);
                add(4, layout.size() as u64);
                add(5, result.is_null() as u64);
                leave();
            }
            result
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            let counted = enter();
            let result = unsafe { System.realloc(ptr, layout, size) };
            if counted {
                add(6, 1);
                add(7, size as u64);
                add(8, layout.size() as u64);
                add(9, result.is_null() as u64);
                leave();
            }
            result
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            let counted = enter();
            unsafe { System.dealloc(ptr, layout) };
            if counted {
                add(10, 1);
                add(11, layout.size() as u64);
                leave();
            }
        }
    }

    pub(crate) fn start() {
        let epoch = EPOCH.load(Ordering::SeqCst);
        assert_eq!(epoch & 1, 0, "allocation phase already active");
        // stop() drained every admitted operation before returning. A delayed
        // rejected entrant can still modify INFLIGHT but cannot update counters.
        // Do not assert INFLIGHT == 0 here: that would reject such a valid race.
        for counter in &COUNTERS {
            counter.store(0, Ordering::Relaxed);
        }
        EPOCH.store(epoch.checked_add(1).unwrap(), Ordering::SeqCst);
    }

    pub(crate) fn stop() -> Option<[u64; 12]> {
        let epoch = EPOCH.load(Ordering::SeqCst);
        if epoch & 1 == 0 {
            return None;
        }
        EPOCH.store(epoch.checked_add(1).unwrap(), Ordering::SeqCst);
        while INFLIGHT.load(Ordering::SeqCst) != 0 {
            std::hint::spin_loop();
        }
        Some(std::array::from_fn(|i| COUNTERS[i].load(Ordering::Relaxed)))
    }

    pub(crate) fn emit(phase: &str, counts: [u64; 12]) {
        println!(
            "{{\"event\":\"allocation_counts\",\"schema\":\"r1-phase-allocation/v1\",\"phase\":\"{phase}\",\"alloc_calls\":{},\"alloc_requested_bytes\":{},\"alloc_failed_calls\":{},\"alloc_zeroed_calls\":{},\"alloc_zeroed_requested_bytes\":{},\"alloc_zeroed_failed_calls\":{},\"realloc_calls\":{},\"realloc_requested_new_bytes\":{},\"realloc_old_layout_bytes\":{},\"realloc_failed_calls\":{},\"dealloc_calls\":{},\"dealloc_layout_bytes\":{}}}",
            counts[0], counts[1], counts[2], counts[3], counts[4], counts[5],
            counts[6], counts[7], counts[8], counts[9], counts[10], counts[11]
        );
    }
}

// Compile this generated standalone source without the solver crate graph.
// It exercises explicit allocator calls, not optimizer-elidable Box values.
fn main() {
    use std::alloc::{GlobalAlloc, Layout};
    use allocation_probe::ALLOCATOR;

    let l32 = Layout::from_size_align(32, 8).unwrap();
    let l64 = Layout::from_size_align(64, 8).unwrap();
    let l128 = Layout::from_size_align(128, 8).unwrap();
    assert!(allocation_probe::stop().is_none());
    unsafe {
        let ignored = ALLOCATOR.alloc(l32);
        assert!(!ignored.is_null());
        ALLOCATOR.dealloc(ignored, l32);
    }

    allocation_probe::start();
    let preserved;
    let zeroed;
    unsafe {
        let old = ALLOCATOR.alloc(l64);
        if old.is_null() {
            std::process::abort();
        }
        old.write_volatile(91);
        let grown = ALLOCATOR.realloc(old, l64, 128);
        if grown.is_null() {
            std::process::abort();
        }
        preserved = grown.read_volatile() == 91;
        ALLOCATOR.dealloc(grown, l128);
        let zero = ALLOCATOR.alloc_zeroed(l32);
        if zero.is_null() {
            std::process::abort();
        }
        zeroed = (0..32).all(|i| zero.add(i).read_volatile() == 0);
        ALLOCATOR.dealloc(zero, l32);
    }
    let first = allocation_probe::stop().unwrap();
    assert!(preserved && zeroed);
    assert_eq!(first, [1, 64, 0, 1, 32, 0, 1, 128, 64, 0, 2, 160]);
    allocation_probe::emit("selftest", first);
    assert!(allocation_probe::stop().is_none());
    allocation_probe::start();
    assert_eq!(allocation_probe::stop().unwrap(), [0; 12]);
    println!("{{\"selftest\":\"passed\",\"null_failure_injection\":false,\"concurrency_stress\":false}}");
}
