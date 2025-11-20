use std::alloc::Layout;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, Default)]
pub struct HeapStats {
    pub total_allocations: u64,
    pub active_allocations: u64,
    pub bytes_in_use: usize,
    pub peak_bytes: usize,
    /// 当前跟踪的 RC 对象数量
    pub rc_tracked_objects: u64,
    /// retain/release 调用次数（调试用）
    pub total_retain_ops: u64,
    pub total_release_ops: u64,
    /// 强引用变为 0 的 release 次数
    pub rc_zero_releases: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct AllocationEvent {
    pub ptr: usize,
    pub layout: Layout,
    pub strong_refs: u64,
}

struct AllocationRegistry {
    records: HashMap<usize, AllocationEvent>,
    stats: HeapStats,
}

impl AllocationRegistry {
    fn new() -> Self {
        Self {
            records: HashMap::new(),
            stats: HeapStats::default(),
        }
    }

    fn register(&mut self, mut event: AllocationEvent) {
        if event.strong_refs == 0 {
            event.strong_refs = 1;
        }
        self.stats.total_allocations += 1;
        self.stats.active_allocations += 1;
        self.stats.bytes_in_use += event.layout.size();
        self.stats.peak_bytes = self.stats.peak_bytes.max(self.stats.bytes_in_use);
        self.stats.rc_tracked_objects += 1;
        self.records.insert(event.ptr, event);
    }

    fn unregister(&mut self, ptr: usize) -> Option<AllocationEvent> {
        if let Some(event) = self.records.remove(&ptr) {
            self.stats.active_allocations = self.stats.active_allocations.saturating_sub(1);
            self.stats.bytes_in_use = self.stats.bytes_in_use.saturating_sub(event.layout.size());
            self.stats.rc_tracked_objects = self.stats.rc_tracked_objects.saturating_sub(1);
            Some(event)
        } else {
            None
        }
    }

    fn retain(&mut self, ptr: usize) -> Option<u64> {
        let record = self.records.get_mut(&ptr)?;
        record.strong_refs = record.strong_refs.saturating_add(1);
        self.stats.total_retain_ops = self.stats.total_retain_ops.saturating_add(1);
        Some(record.strong_refs)
    }

    fn release(&mut self, ptr: usize) -> Option<ReleaseOutcome> {
        let should_finalize = {
            let record = self.records.get_mut(&ptr)?;
            self.stats.total_release_ops = self.stats.total_release_ops.saturating_add(1);
            if record.strong_refs > 1 {
                record.strong_refs -= 1;
                return Some(ReleaseOutcome::StillAlive(record.strong_refs));
            }
            true
        };

        if should_finalize {
            let event = self.unregister(ptr)?;
            self.stats.rc_zero_releases = self.stats.rc_zero_releases.saturating_add(1);
            return Some(ReleaseOutcome::ShouldFree(event.layout));
        }
        None
    }
}

fn registry() -> &'static Mutex<AllocationRegistry> {
    static REGISTRY: OnceLock<Mutex<AllocationRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(AllocationRegistry::new()))
}

pub fn register_allocation(ptr: *mut u8, layout: Layout) {
    if ptr.is_null() {
        return;
    }
    let mut guard = registry().lock().expect("allocation registry poisoned");
    guard.register(AllocationEvent {
        ptr: ptr as usize,
        layout,
        strong_refs: 1,
    });
}

pub fn unregister_allocation(ptr: u64) -> Option<Layout> {
    if ptr == 0 {
        return None;
    }
    let mut guard = registry().lock().expect("allocation registry poisoned");
    guard.unregister(ptr as usize).map(|event| event.layout)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseOutcome {
    StillAlive(u64),
    ShouldFree(Layout),
}

pub fn retain_allocation(ptr: u64) -> Option<u64> {
    if ptr == 0 {
        return None;
    }
    let mut guard = registry().lock().expect("allocation registry poisoned");
    guard.retain(ptr as usize)
}

pub fn release_allocation(ptr: u64) -> Option<ReleaseOutcome> {
    if ptr == 0 {
        return None;
    }
    let mut guard = registry().lock().expect("allocation registry poisoned");
    guard.release(ptr as usize)
}

pub fn current_stats() -> HeapStats {
    let guard = registry().lock().expect("allocation registry poisoned");
    guard.stats
}
