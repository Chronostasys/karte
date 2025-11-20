use std::alloc::{alloc, alloc_zeroed, dealloc, realloc, Layout};
use std::ptr::{copy_nonoverlapping, null_mut};
use std::sync::{OnceLock, RwLock};

/// 运行时可自定义的分配器接口
pub trait Allocator: Send + Sync + 'static {
    fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError>;

    fn alloc_zeroed(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        let ptr = self.alloc(layout)?;
        unsafe {
            ptr.write_bytes(0, layout.size());
        }
        Ok(ptr)
    }

    fn realloc(
        &self,
        ptr: *mut u8,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<*mut u8, AllocError> {
        if ptr.is_null() {
            return self.alloc(new_layout);
        }

        unsafe {
            let new_ptr = self.alloc(new_layout)?;
            let copy_size = old_layout.size().min(new_layout.size());
            copy_nonoverlapping(ptr, new_ptr, copy_size);
            self.dealloc(ptr, old_layout);
            Ok(new_ptr)
        }
    }

    fn dealloc(&self, ptr: *mut u8, layout: Layout);
}

/// 系统分配器（默认实现，基于 `std::alloc`）
#[derive(Debug, Default)]
pub struct SystemAllocator;

impl Allocator for SystemAllocator {
    fn alloc(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe {
            let ptr = alloc(layout);
            if ptr.is_null() {
                Err(AllocError::OutOfMemory { layout })
            } else {
                Ok(ptr)
            }
        }
    }

    fn alloc_zeroed(&self, layout: Layout) -> Result<*mut u8, AllocError> {
        unsafe {
            let ptr = alloc_zeroed(layout);
            if ptr.is_null() {
                Err(AllocError::OutOfMemory { layout })
            } else {
                Ok(ptr)
            }
        }
    }

    fn realloc(
        &self,
        ptr: *mut u8,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<*mut u8, AllocError> {
        unsafe {
            let ptr = realloc(ptr, old_layout, new_layout.size());
            if ptr.is_null() {
                Err(AllocError::OutOfMemory { layout: new_layout })
            } else {
                Ok(ptr)
            }
        }
    }

    fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if ptr.is_null() {
            return;
        }
        unsafe {
            dealloc(ptr, layout);
        }
    }
}

/// 统一封装的布局请求
#[derive(Debug, Clone, Copy)]
pub struct LayoutRequest {
    pub size: usize,
    pub align: usize,
}

impl LayoutRequest {
    pub fn sanitize(self) -> Self {
        Self {
            size: self.size.max(8),
            align: self.align.max(8).next_power_of_two(),
        }
    }

    pub fn to_layout(self) -> Result<Layout, AllocError> {
        let req = self.sanitize();
        Layout::from_size_align(req.size, req.align)
            .map_err(|_| AllocError::InvalidLayout { request: req })
    }
}

/// 分配错误
#[derive(Debug, Clone, Copy)]
pub enum AllocError {
    InvalidLayout { request: LayoutRequest },
    OutOfMemory { layout: Layout },
}

/// 用于访问/替换全局分配器的句柄
pub struct AllocatorHandle(&'static RwLock<Box<dyn Allocator>>);

impl AllocatorHandle {
    pub fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&dyn Allocator) -> R,
    {
        let guard = self.0.read().expect("allocator poisoned");
        f(guard.as_ref())
    }
}

impl Default for AllocatorHandle {
    fn default() -> Self {
        AllocatorHandle(allocator_slot())
    }
}

/// 设置新的全局分配器实现
pub fn set_allocator(new_allocator: Box<dyn Allocator>) {
    let slot = allocator_slot();
    let mut guard = slot.write().expect("allocator poisoned");
    *guard = new_allocator;
}

fn allocator_slot() -> &'static RwLock<Box<dyn Allocator>> {
    static INSTANCE: OnceLock<RwLock<Box<dyn Allocator>>> = OnceLock::new();
    INSTANCE.get_or_init(|| RwLock::new(Box::new(SystemAllocator) as Box<dyn Allocator>))
}

/// 快捷访问全局分配器
pub fn global_allocator() -> AllocatorHandle {
    AllocatorHandle::default()
}

/// 工具函数：空指针表示分配失败
pub fn null_ptr() -> *mut u8 {
    null_mut()
}
