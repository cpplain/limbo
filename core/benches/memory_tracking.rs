// Memory tracking utilities for benchmarks
// This can be integrated into benchmarks to measure memory usage

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct TrackingAllocator;

static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static DEALLOCATED: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ret = System.alloc(layout);
        if !ret.is_null() {
            ALLOCATED.fetch_add(layout.size(), Ordering::SeqCst);
        }
        ret
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        DEALLOCATED.fetch_add(layout.size(), Ordering::SeqCst);
    }
}

pub fn reset_memory_stats() {
    ALLOCATED.store(0, Ordering::SeqCst);
    DEALLOCATED.store(0, Ordering::SeqCst);
}

pub fn get_memory_stats() -> (usize, usize) {
    (
        ALLOCATED.load(Ordering::SeqCst),
        DEALLOCATED.load(Ordering::SeqCst),
    )
}

pub fn get_net_memory_usage() -> isize {
    let allocated = ALLOCATED.load(Ordering::SeqCst) as isize;
    let deallocated = DEALLOCATED.load(Ordering::SeqCst) as isize;
    allocated - deallocated
}

// Usage in benchmarks:
// 
// #[global_allocator]
// static GLOBAL: TrackingAllocator = TrackingAllocator;
//
// Then in benchmark:
// reset_memory_stats();
// // ... run benchmark code ...
// let memory_used = get_net_memory_usage();