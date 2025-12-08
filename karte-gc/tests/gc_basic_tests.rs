//! Karte GC 基础功能测试

use karte_gc::{gc_alloc, gc_trigger_collect, initialize_gc, ObjectType};

/// 测试 GC 初始化
#[test]
fn test_gc_initialization() {
    unsafe {
        // 初始化 GC
        initialize_gc();

        // 如果没有崩溃，说明初始化成功
        println!("GC initialized successfully");
    }
}

/// 测试简单的内存分配
#[test]
fn test_simple_allocation() {
    unsafe {
        initialize_gc();

        // 分配一个原子类型对象（64字节）
        let ptr = gc_alloc(64, ObjectType::Atomic);
        assert!(!ptr.is_null(), "Allocation should succeed");

        // 写入一些数据
        std::ptr::write_bytes(ptr, 0xAB, 64);

        // 读取验证
        let first_byte = *ptr;
        assert_eq!(first_byte, 0xAB);

        println!("Allocated and wrote to {:p}", ptr);
    }
}

/// 测试多次分配
#[test]
fn test_multiple_allocations() {
    unsafe {
        initialize_gc();

        // 测试连续多次分配都能成功
        let count = 50;

        for i in 0..count {
            let size = 64 + (i % 32) * 8; // 变化大小
            let obj_type = match i % 3 {
                0 => ObjectType::Atomic,
                1 => ObjectType::Pointer,
                _ => ObjectType::Conservative,
            };

            let ptr = gc_alloc(size, obj_type);
            assert!(!ptr.is_null(), "Allocation {} should succeed", i);

            // 写入并立即验证
            *ptr = i as u8;
            assert_eq!(
                *ptr, i as u8,
                "Immediate verification failed for allocation {}",
                i
            );
        }

        println!("Successfully completed {} allocations", count);
    }
}

/// 测试 GC 收集
#[test]
fn test_gc_collection() {
    unsafe {
        initialize_gc();

        // 分配一些对象
        for i in 0..50 {
            let ptr = gc_alloc(128, ObjectType::Conservative);
            assert!(!ptr.is_null());

            // 写入数据
            std::ptr::write_bytes(ptr, i as u8, 128);
        }

        // 手动触发 GC
        gc_trigger_collect();

        // 如果没有崩溃，说明 GC 工作正常
        println!("GC collection completed successfully");

        // 分配后测试，确保 GC 后仍然可以分配
        let ptr = gc_alloc(256, ObjectType::Conservative);
        assert!(!ptr.is_null(), "Post-GC allocation should succeed");
    }
}

/// 测试大对象分配
#[test]
fn test_large_object_allocation() {
    unsafe {
        initialize_gc();

        // 分配一个大对象（1MB）
        let large_size = 1024 * 1024;
        let ptr = gc_alloc(large_size, ObjectType::Conservative);

        assert!(!ptr.is_null(), "Large object allocation should succeed");

        // 写入模式
        for i in 0..1024 {
            *ptr.add(i * 1024) = (i % 256) as u8;
        }

        // 验证
        for i in 0..1024 {
            let value = *ptr.add(i * 1024);
            assert_eq!(value, (i % 256) as u8);
        }

        println!("Successfully allocated and verified 1MB object");
    }
}

/// 测试不同对象类型的分配
#[test]
fn test_object_types() {
    unsafe {
        initialize_gc();

        // Atomic 类型
        let atomic_ptr = gc_alloc(64, ObjectType::Atomic);
        assert!(!atomic_ptr.is_null());
        println!("Allocated Atomic object at {:p}", atomic_ptr);

        // Pointer 类型
        let pointer_ptr = gc_alloc(64, ObjectType::Pointer);
        assert!(!pointer_ptr.is_null());
        println!("Allocated Pointer object at {:p}", pointer_ptr);

        // Conservative 类型
        let conservative_ptr = gc_alloc(128, ObjectType::Conservative);
        assert!(!conservative_ptr.is_null());
        println!("Allocated Conservative object at {:p}", conservative_ptr);

        // 所有类型都应该成功分配
    }
}

/// 压力测试：大量分配和GC
#[test]
fn test_allocation_stress() {
    unsafe {
        initialize_gc();

        // 多轮分配和GC
        for round in 0..5 {
            println!("Stress test round {}", round);

            // 每轮分配1000个对象
            for _ in 0..1000 {
                let size = 64 + (round * 16);
                let ptr = gc_alloc(size, ObjectType::Conservative);
                assert!(!ptr.is_null());
            }

            // 触发GC
            gc_trigger_collect();
        }

        println!("Stress test completed: 5000 allocations with 5 GC collections");
    }
}
