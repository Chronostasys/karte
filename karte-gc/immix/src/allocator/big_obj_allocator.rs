use std::cell::RefCell;

use parking_lot::ReentrantMutex;

use crate::{bigobj::BigObj, mmap::Mmap, round_n_up, ALIGN, BIG_OBJ_ALIGN};

pub struct BigObjAllocator {
    mmap: Mmap,
    current: *mut u8,
    heap_start: *mut u8,
    heap_end: *mut u8,
    unused_chunks: Vec<*mut BigObj>,
    lock: ReentrantMutex<()>,
    commited_to: RefCell<*mut u8>,
}

impl BigObjAllocator {
    pub fn new(size: usize) -> Self {
        let mmap = Mmap::new(size);
        Self {
            current: mmap.aligned(ALIGN),
            heap_start: mmap.aligned(ALIGN),
            commited_to: RefCell::new(mmap.aligned(ALIGN)),
            heap_end: mmap.end(),
            mmap,
            unused_chunks: Vec::new(),
            lock: ReentrantMutex::new(()),
        }
    }

    pub fn state(&self) {
        println!("current: {:p}", self.current);
        println!("heap_start: {:p}", self.heap_start);
        println!("heap_end: {:p}", self.heap_end);
    }

    pub fn size(&self) -> usize {
        self.heap_end as usize - self.heap_start as usize
    }

    pub fn alloc_chunk(&self, size: usize) -> Option<*mut BigObj> {
        let _lock = self.lock.lock();
        let current = self.current;
        let heap_end = self.heap_end;
        let next = unsafe { current.add(size) };

        if next >= heap_end {
            // heap 已满，返回 None 让上层触发 emergency GC
            return None;
        }
        unsafe {
            let end = current.add(size);
            let offset = current.align_offset(ALIGN);
            let dest = *self.commited_to.borrow();

            if end > dest && !self.mmap.commit(dest, size - offset) {
                return None;
            }
            self.commited_to.replace(end.add(end.align_offset(ALIGN)));
        }

        let obj = BigObj::new(current, size);
        Some(obj)
    }

    pub fn get_chunk(&mut self, size: usize) -> *mut BigObj {
        let size = round_n_up!(size + 16, BIG_OBJ_ALIGN); // 16 is the size of BigObj
        let _lock = self.lock.lock();
        for i in 0..self.unused_chunks.len() {
            let unused_obj = self.unused_chunks[i];
            let unused_size = unsafe { (*unused_obj).size };
            match unused_size.cmp(&size) {
                std::cmp::Ordering::Less => {}
                std::cmp::Ordering::Equal => {
                    self.unused_chunks.remove(i);
                    return unused_obj;
                }
                std::cmp::Ordering::Greater => {
                    let ptr = unsafe { (unused_obj as *mut u8).add(unused_size - size) };
                    debug_assert!(ptr as usize % BIG_OBJ_ALIGN == 0);
                    let new_obj = BigObj::new(ptr, size);
                    unsafe {
                        (*unused_obj).size -= size;
                    }
                    return new_obj;
                }
            };
        }

        // 没有合适的可复用 chunk，从 bump allocator 分配新的
        match self.alloc_chunk(size) {
            Some(chunk) => {
                unsafe { self.current = self.current.add(size) };
                log::trace!("get_chunk: {:p}[new {}]", chunk, size);
                chunk
            }
            None => {
                // 返回空指针，让上层触发 emergency GC
                std::ptr::null_mut()
            }
        }
    }

    pub fn return_chunk(&mut self, obj: *mut BigObj) {
        let _lock = self.lock.lock();
        let size = unsafe { (*obj).size };
        log::trace!("ret_chunk: {:p}[size {}]", obj, size);
        let mut merged = false;
        // 合并相邻 free_obj
        let mut i = 0;
        while i < self.unused_chunks.len() {
            let unused_obj = self.unused_chunks[i];
            let unused_obj_ptr = unused_obj as *mut u8;
            let unused_size = unsafe { (*unused_obj).size };
            if unsafe { unused_obj_ptr.sub(size) } == obj as *mut u8 {
                // |    return_obj    |  unused_obj  |
                unsafe {
                    (*obj).size += unused_size;
                }
                self.unused_chunks.remove(i);
                self.unused_chunks.push(obj);
                merged = true;
                break;
            } else if unsafe { unused_obj_ptr.add(unused_size) } == obj as *mut u8 {
                // |  unused_obj  |    return_obj    |
                // 修复：使用 unused_obj 的 size 来判断邻接
                unsafe {
                    (*unused_obj).size += size;
                }
                merged = true;
                break;
            }
            i += 1;
        }
        if !merged {
            self.unused_chunks.push(obj);
        }
    }

    /// # in_heap
    ///
    /// Check if a pointer is in the heap.
    pub fn in_heap(&self, ptr: *mut u8) -> bool {
        ptr >= self.heap_start && ptr < self.heap_end
    }
}
