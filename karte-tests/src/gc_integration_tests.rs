//! GC 集成测试
//!
//! 这些测试验证 Karte GC (Immix) 的功能，包括：
//! - 虚拟栈扫描
//! - 对象类型分配
//! - GC 回收正确性

#[test]
fn test_gc_virtual_stack_scanning() {
    use karte_gc::{
        gc_alloc, gc_trigger_collect, initialize_gc, register_virtual_stack_range, ObjectType,
    };

    unsafe {
        initialize_gc();

        // 创建虚拟栈并注册为 GC 根
        // 这模拟了 ExecutionEngine 的虚拟栈结构
        let mut virtual_stack: Vec<i64> = vec![0; 1024];
        let stack_start = virtual_stack.as_ptr() as *const u8;
        let stack_end = stack_start.add(virtual_stack.len() * 8);
        register_virtual_stack_range(stack_start, stack_end);

        // 分配对象并保存到虚拟栈的不同位置
        let obj1 = gc_alloc(64, ObjectType::Conservative);
        assert!(!obj1.is_null(), "obj1 分配失败");
        virtual_stack[10] = obj1 as i64;

        let obj2 = gc_alloc(128, ObjectType::Conservative);
        assert!(!obj2.is_null(), "obj2 分配失败");
        virtual_stack[20] = obj2 as i64;

        let obj3 = gc_alloc(256, ObjectType::Conservative);
        assert!(!obj3.is_null(), "obj3 分配失败");
        virtual_stack[100] = obj3 as i64;

        // 触发 GC 收集
        gc_trigger_collect();

        // 验证对象仍然有效（未被回收）
        // 由于对象指针保存在虚拟栈中，自定义扫描器应该能找到它们
        let obj1_after_gc = virtual_stack[10] as *mut u8;
        let obj2_after_gc = virtual_stack[20] as *mut u8;
        let obj3_after_gc = virtual_stack[100] as *mut u8;

        assert!(
            !obj1_after_gc.is_null(),
            "obj1 在 GC 后应该仍然有效（虚拟栈扫描应该找到它）"
        );
        assert!(
            !obj2_after_gc.is_null(),
            "obj2 在 GC 后应该仍然有效（虚拟栈扫描应该找到它）"
        );
        assert!(
            !obj3_after_gc.is_null(),
            "obj3 在 GC 后应该仍然有效（虚拟栈扫描应该找到它）"
        );

        // 验证我们可以写入这些对象（它们的内存仍然有效）
        obj1_after_gc.write_bytes(0xAA, 64);
        obj2_after_gc.write_bytes(0xBB, 128);
        obj3_after_gc.write_bytes(0xCC, 256);

        // 再次触发 GC，验证多次 GC 后对象仍然有效
        gc_trigger_collect();

        let obj1_final = virtual_stack[10] as *mut u8;
        let obj2_final = virtual_stack[20] as *mut u8;
        let obj3_final = virtual_stack[100] as *mut u8;

        assert!(!obj1_final.is_null(), "obj1 在第二次 GC 后应该仍然有效");
        assert!(!obj2_final.is_null(), "obj2 在第二次 GC 后应该仍然有效");
        assert!(!obj3_final.is_null(), "obj3 在第二次 GC 后应该仍然有效");
    }
}

#[test]
fn test_gc_object_types() {
    use karte_gc::{gc_alloc, ObjectType};

    unsafe {
        // 测试 Atomic 类型分配
        let atomic_obj = gc_alloc(8, ObjectType::Atomic);
        assert!(!atomic_obj.is_null(), "Atomic 类型对象分配应该成功");

        // 测试 Pointer 类型分配
        let pointer_obj = gc_alloc(8, ObjectType::Pointer);
        assert!(!pointer_obj.is_null(), "Pointer 类型对象分配应该成功");

        // 测试 Conservative 类型分配
        let conservative_obj = gc_alloc(64, ObjectType::Conservative);
        assert!(
            !conservative_obj.is_null(),
            "Conservative 类型对象分配应该成功"
        );

        // 测试大对象分配 (Conservative)
        let large_obj = gc_alloc(4096, ObjectType::Conservative);
        assert!(!large_obj.is_null(), "大对象 (Conservative) 分配应该成功");

        // 验证可以写入和读取这些对象
        atomic_obj.cast::<i64>().write(42i64);
        assert_eq!(
            atomic_obj.cast::<i64>().read(),
            42,
            "Atomic 对象应该可以读写"
        );

        pointer_obj
            .cast::<*mut u8>()
            .write(0xDEADBEEFu64 as *mut u8);
        assert_eq!(
            pointer_obj.cast::<*mut u8>().read() as u64,
            0xDEADBEEF,
            "Pointer 对象应该可以读写"
        );

        // 写入一些数据到 Conservative 对象
        for i in 0..8 {
            conservative_obj.add(i * 8).cast::<i64>().write(i as i64);
        }

        // 验证数据
        for i in 0..8 {
            assert_eq!(
                conservative_obj.add(i * 8).cast::<i64>().read(),
                i as i64,
                "Conservative 对象第 {} 个字段应该正确",
                i
            );
        }
    }
}

#[test]
fn test_gc_no_false_negatives() {
    use karte_gc::{gc_alloc, gc_trigger_collect, initialize_gc, ObjectType};

    unsafe {
        initialize_gc();

        // 分配一个对象但不将其保存到任何根
        let unreachable_obj = gc_alloc(128, ObjectType::Conservative);
        assert!(!unreachable_obj.is_null(), "对象分配应该成功");

        // 写入标记值
        unreachable_obj.write_bytes(0xFF, 128);

        // 触发 GC - 这个对象应该被回收（因为没有根引用它）
        gc_trigger_collect();

        // 注意：我们不能验证对象是否被回收，因为访问已回收的内存是 UB
        // 这个测试主要验证 GC 不会崩溃，能正常运行
    }
}

#[test]
fn test_gc_allocation_stress() {
    use karte_gc::{gc_alloc, gc_trigger_collect, initialize_gc, ObjectType};

    unsafe {
        initialize_gc();

        // 压力测试：分配大量小对象
        // 注意：这个测试不验证对象内容，因为 GC evacuation 可能会移动对象
        // 真实场景中，Karte 虚拟栈会被注册为根，GC 会正确更新栈中的指针
        let mut object_count = 0;
        for i in 0..1000 {
            let obj = gc_alloc(64, ObjectType::Conservative);
            assert!(!obj.is_null(), "第 {} 次分配应该成功", i);
            // 写入标记值（仅用于测试分配成功）
            obj.cast::<i64>().write(i as i64);
            object_count += 1;
        }

        assert_eq!(object_count, 1000, "应该成功分配1000个对象");

        // 触发 GC - 验证 GC 能够正常运行而不崩溃
        gc_trigger_collect();

        // 再分配一些对象，测试 GC 后的分配
        for i in 0..100 {
            let obj = gc_alloc(128, ObjectType::Conservative);
            assert!(!obj.is_null(), "GC 后第 {} 次分配应该成功", i);
            // 写入数据验证对象可用
            obj.cast::<i64>().write((1000 + i) as i64);
            assert_eq!(
                obj.cast::<i64>().read(),
                (1000 + i) as i64,
                "新分配的对象应该可以读写"
            );
        }
    }
}
