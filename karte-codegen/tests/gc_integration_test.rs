//! GC 集成测试
//!
//! 验证 ExecutionEngine 与 GC 的集成是否正常工作

use karte_codegen::vm::professional_executor::{ExecutionEngine, ProgramManager};

#[test]
fn test_gc_initialization_with_execution_engine() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init()
        .ok();

    // 创建执行引擎
    let mut engine = ExecutionEngine::new(true);

    // 创建一个简单的程序管理器
    let program_manager = ProgramManager::new();

    // 初始化执行环境（这应该会初始化 GC）
    let result = engine.initialize(&program_manager);

    // 验证初始化成功
    assert!(result.is_ok(), "执行引擎初始化失败: {:?}", result.err());

    println!("✅ GC 集成测试通过：ExecutionEngine 初始化成功");
}

#[test]
fn test_gc_with_simple_allocation() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init()
        .ok();

    // 创建执行引擎
    let mut engine = ExecutionEngine::new(false);

    // 创建程序管理器
    let program_manager = ProgramManager::new();

    // 初始化（包括 GC 初始化）
    engine.initialize(&program_manager).expect("初始化失败");

    // 测试堆分配（这将使用 GC）
    let result = engine.allocate_heap(64, "test");

    assert!(result.is_ok(), "堆分配失败: {:?}", result.err());

    let addr = result.unwrap();
    assert!(addr > 0, "分配的地址应该非零");

    println!("✅ GC 堆分配测试通过：成功分配地址 0x{:x}", addr);
}

#[test]
fn test_gc_with_multiple_allocations() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .is_test(true)
        .try_init()
        .ok();

    // 创建执行引擎
    let mut engine = ExecutionEngine::new(false);

    // 创建程序管理器
    let program_manager = ProgramManager::new();

    // 初始化
    engine.initialize(&program_manager).expect("初始化失败");

    // 多次分配
    let mut addresses = Vec::new();
    for i in 0..10 {
        let result = engine.allocate_heap(64 + i * 8, "test_object");
        assert!(result.is_ok(), "第 {} 次分配失败: {:?}", i, result.err());

        let addr = result.unwrap();
        assert!(addr > 0, "第 {} 次分配的地址应该非零", i);
        addresses.push(addr);

        println!("分配 #{}: 地址=0x{:x}, 大小={}", i, addr, 64 + i * 8);
    }

    // 验证所有地址都不同
    for i in 0..addresses.len() {
        for j in (i + 1)..addresses.len() {
            assert_ne!(addresses[i], addresses[j], "地址 {} 和 {} 不应该相同", i, j);
        }
    }

    println!(
        "✅ 多次堆分配测试通过：成功分配 {} 个不同的地址",
        addresses.len()
    );
}
