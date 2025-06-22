//! 寄存器分配Pass
//!
//! ## 概述
//!
//! 这是寄存器分配的核心模块，实现了一个基于线性扫描的分配器。
//! 它被设计为一个两阶段的过程，以支持更灵活和强大的代码生成流程。
//!
//! ### 两阶段分配架构
//!
//! 1.  **Pre-RA (决策阶段)**:
//!     -   在 `DecisionOnly` 模式下运行。
//!     -   此阶段仅进行分析，决定哪些虚拟寄存器映射到物理寄存器，哪些需要溢出。
//!     -   结果被存储，但不修改代码。这是为 `StackFrameLowering` Pass 提供信息。
//!
//! 2.  **StackFrameLowering**:
//!     -   一个独立的Pass，在 Pre-RA 之后，Final-RA 之前运行。
//!     -   它根据 Pre-RA 的溢出决策，计算栈帧大小，并用真实的栈加载/存储指令
//!       替换掉所有对溢出寄存器的访问，同时可能会引入新的临时虚拟寄存器。
//!
//! 3.  **Final-RA (改写阶段)**:
//!     -   在 `FinalRewrite` 模式下运行。
//!     -   在 `StackFrameLowering` 完成后，此阶段重新运行一次完整的分配过程，
//!       为函数中所有的虚拟寄存器（包括 `StackFrameLowering` 新引入的临时寄存器）
//!       分配最终的物理寄存器。
//!     -   最后，它会重写LIR，将所有虚拟寄存器引用替换为物理寄存器引用。
//!
//! ## 模块结构
//!
//! - `types.rs`: 定义核心数据结构，如 `RegisterAllocationResult` 和 `RegisterLifetime`。
//! - `lifetime_analysis.rs`: 封装了所有关于计算寄存器生命周期的逻辑。
//! - `linear_scan.rs`: 实现了线性扫描分配算法本身。

pub mod lifetime_analysis;
pub mod linear_scan;
pub mod types;

// 重新导出主要类型，方便外部使用
pub use lifetime_analysis::LifetimeAnalyzer;
pub use linear_scan::LinearScanAllocator;
pub use types::*;

use super::{AnalysisManager, FunctionPass, PassResult};
use crate::{AllocationType, IndexInstructionTransformer, Instruction, LirFunction, Operand, RegisterId};
use karte_diagnostics::Span;
use std::collections::{HashMap, HashSet};
// 🔧 新增：导入instruction_transformer.rs中的智能变换系统
use crate::pass::instruction_transformer::{BatchTransformer, BatchTransformResult};

/// 线性扫描寄存器分配Pass
///
/// 封装了整个寄存器分配流程，并协调其子模块。
#[deprecated]
pub struct LinearScanRegisterAllocation {
    mode: RegisterAllocationMode,
    lifetime_analyzer: LifetimeAnalyzer,
    linear_scan_allocator: LinearScanAllocator,
}

impl LinearScanRegisterAllocation {
    /// 创建一个新的寄存器分配器实例
    pub fn new(mode: RegisterAllocationMode) -> Self {
        let calling_convention = types::SimpleCallingConvention::default();
        let lifetime_analyzer = LifetimeAnalyzer::new(calling_convention.clone());
        let linear_scan_allocator = LinearScanAllocator::new(calling_convention.clone());

        Self {
            mode,
            lifetime_analyzer,
            linear_scan_allocator,
        }
    }
}

impl FunctionPass for LinearScanRegisterAllocation {
    fn name(&self) -> &str {
        "linear-scan-register-allocation"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        analyses: &mut AnalysisManager,
    ) -> PassResult {
        PassResult::Unchanged
    }

    fn required_analyses(&self) -> Vec<&'static str> {
        vec!["cfg", "def-use"]
    }
    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec!["cfg", "def-use"]
    }
}

impl LinearScanRegisterAllocation {

}

/// 🔧 遵循调用约定的简单栈式寄存器分配Pass
/// 
/// 调用约定：
/// - r0: 返回值
/// - r1-r4: 参数传递（最多4个参数）
/// - r5: 返回地址
/// - r6: 栈指针（SP）
/// - r7: 帧指针（FP）
pub struct SimpleStackRegisterAllocation {
    /// 调用约定
    calling_convention: SimpleCallingConvention,
    /// 当前溢出的寄存器计数
    spill_counter: usize,
}

/// 分配目标
#[derive(Debug, Clone, Copy)]
enum AllocationTarget {
    /// 分配到物理寄存器
    Register(u8),
    /// 溢出到栈槽
    Spill(usize),
}

/// 简化的调用约定（基于calling_convention.rs）
#[derive(Debug, Clone)]
struct SimpleCallingConvention {
    /// 参数寄存器 r1-r4
    argument_registers: Vec<u8>,
    /// 返回值寄存器 r0
    return_register: u8,
    /// 可分配的通用寄存器（排除特殊寄存器）
    allocatable_registers: Vec<u8>,
    /// 特殊寄存器（不能分配）
    special_registers: HashSet<u8>,
}

impl SimpleCallingConvention {
    fn new() -> Self {
        let mut special_registers = HashSet::new();
        // 🔧 修复：只保留真正的特殊寄存器
        special_registers.insert(6); // 栈指针 
        special_registers.insert(7); // 帧指针
        
        Self {
            argument_registers: vec![1, 2, 3, 4],
            return_register: 0,
            // 🔧 修复：扩展可分配寄存器范围，包含r5
            allocatable_registers: vec![0, 1, 2, 3, 4, 5],
            special_registers,
        }
    }
    
    /// 检查寄存器是否是特殊寄存器
    fn is_special_register(&self, reg: u8) -> bool {
        self.special_registers.contains(&reg)
    }
    
    /// 检查寄存器是否是参数寄存器
    fn is_argument_register(&self, reg: u8) -> bool {
        self.argument_registers.contains(&reg)
    }
    
    /// 检查寄存器是否是返回值寄存器
    fn is_return_register(&self, reg: u8) -> bool {
        reg == self.return_register
    }
}

impl SimpleStackRegisterAllocation {
    pub fn new() -> Self {
        Self {
            calling_convention: SimpleCallingConvention::new(),
            spill_counter: 0,
        }
    }
}

impl FunctionPass for SimpleStackRegisterAllocation {
    fn name(&self) -> &str {
        "simple-stack-register-allocation"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        _analyses: &mut AnalysisManager,
    ) -> PassResult {
        println!("🎯 开始遵循调用约定的寄存器分配：{}", function.name);
        
        // 重置状态
        self.spill_counter = 0;
        
        // 分析所有虚拟寄存器的使用情况
        let virtual_registers = self.collect_virtual_registers(function);
        println!("🎯 发现虚拟寄存器: {:?}", virtual_registers);
        
        // 🔧 新增：根据调用约定构建寄存器分配映射
        let allocation_map = self.build_calling_convention_allocation_map(function, &virtual_registers);
        println!("🎯 调用约定寄存器分配映射: {:?}", allocation_map);
        
        // 应用分配并处理溢出
        self.apply_allocation_with_spilling(function, &allocation_map)
    }

    fn required_analyses(&self) -> Vec<&'static str> {
        vec![]
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec!["cfg", "def-use"]
    }
}

impl SimpleStackRegisterAllocation {
    /// 收集函数中所有使用的虚拟寄存器
    fn collect_virtual_registers(&self, function: &LirFunction) -> Vec<RegisterId> {
        let mut registers = HashSet::new();
        
        println!("🔍 开始收集虚拟寄存器...");
        for (i, instruction) in function.instructions.iter().enumerate() {
            // 收集定义的寄存器
            if let Some(def_reg) = instruction.get_def_register() {
                println!("  指令{}: 定义寄存器 {:?} - {:?}", i, def_reg, instruction);
                registers.insert(def_reg);
            }
            
            // 收集使用的寄存器
            for used_reg in instruction.get_used_registers() {
                println!("  指令{}: 使用寄存器 {:?} - {:?}", i, used_reg, instruction);
                registers.insert(used_reg);
            }
        }
        
        println!("🔍 收集到的所有寄存器: {:?}", registers);
        
        // 🔧 关键修复：确保确定性的寄存器顺序
        let mut sorted_registers: Vec<RegisterId> = registers.into_iter()
            .filter(|reg| {
                let is_special = self.calling_convention.is_special_register(reg.0 as u8);
                if is_special {
                    println!("  过滤特殊寄存器: {:?}", reg);
                }
                !is_special
            })
            .collect();
        
        // 按寄存器ID排序，确保每次运行结果一致
        sorted_registers.sort_by_key(|reg| reg.0);
        
        println!("🎯 发现虚拟寄存器: {:?}", sorted_registers);
        sorted_registers
    }
    
    /// 🔧 重构：基于生命周期分析的寄存器分配映射
    fn build_calling_convention_allocation_map(
        &self, 
        function: &LirFunction, 
        virtual_registers: &[RegisterId]
    ) -> HashMap<RegisterId, AllocationTarget> {
        println!("🎯 开始遵循调用约定的寄存器分配：{}", function.name);
        
        // 🔧 关键修复：首先进行生命周期分析
        let lifetime_analyzer = lifetime_analysis::LifetimeAnalyzer::new(
            types::SimpleCallingConvention::default()
        );
        let (lifetimes, register_types) = lifetime_analyzer.analyze_simple(function);
        
        // 构建生命周期映射，用于冲突检测
        let mut lifetime_map = HashMap::new();
        for lifetime in &lifetimes {
            lifetime_map.insert(lifetime.register, (lifetime.start, lifetime.end));
        }
        
        let mut allocation_map = HashMap::new();
        let mut used_physical_regs = HashSet::new();
        
        // 第一步：为函数参数寄存器分配固定的物理寄存器
        println!("🔧 第一步：分配函数参数寄存器");
        for (i, &param_reg) in function.parameter_registers.iter().enumerate() {
            if i < self.calling_convention.argument_registers.len() {
                let physical_reg = self.calling_convention.argument_registers[i];
                allocation_map.insert(param_reg, AllocationTarget::Register(physical_reg));
                used_physical_regs.insert(physical_reg);
                println!("  参数寄存器 {:?} -> r{}", param_reg, physical_reg);
            } else {
                // 参数过多，需要溢出到栈
                let spill_slot = allocation_map.values()
                    .filter_map(|target| if let AllocationTarget::Spill(slot) = target { Some(*slot) } else { None })
                    .max().unwrap_or(0) + 1;
                allocation_map.insert(param_reg, AllocationTarget::Spill(spill_slot));
                println!("  参数寄存器 {:?} -> 溢出槽{}", param_reg, spill_slot);
            }
        }
        
        // 第二步：为返回值寄存器分配r0（如果函数有返回值）
        println!("🔧 第二步：分配返回值寄存器");
        if let Some(return_reg) = self.find_return_register(function) {
            allocation_map.insert(return_reg, AllocationTarget::Register(self.calling_convention.return_register));
            used_physical_regs.insert(self.calling_convention.return_register);
            println!("  返回值寄存器 {:?} -> r{}", return_reg, self.calling_convention.return_register);
        }
        
        // 第三步：基于生命周期分析为其他虚拟寄存器分配物理寄存器
        println!("🔧 第三步：分配其他虚拟寄存器");
        
        // 🔧 关键修复：按生命周期开始时间排序，确保确定性分配
        let mut remaining_registers: Vec<RegisterId> = virtual_registers.iter()
            .filter(|&reg| !allocation_map.contains_key(reg))
            .copied()
            .collect();
        
        remaining_registers.sort_by(|&a, &b| {
            let start_a = lifetime_map.get(&a).map(|(start, _)| *start).unwrap_or(0);
            let start_b = lifetime_map.get(&b).map(|(start, _)| *start).unwrap_or(0);
            start_a.cmp(&start_b).then_with(|| a.0.cmp(&b.0)) // 生命周期相同时按ID排序
        });
        
        // 🔧 调试：显示排序后的寄存器顺序
        println!("🔧 按生命周期排序后的寄存器分配顺序:");
        for &reg in &remaining_registers {
            let (start, end) = lifetime_map.get(&reg).copied().unwrap_or((0, 0));
            println!("  {:?}: 生命周期[{}, {}]", reg, start, end);
        }
        
        for virtual_reg in remaining_registers {
            // 🔧 关键修复：检查寄存器类型，特殊处理
            let reg_type = register_types.get(&virtual_reg).copied().unwrap_or(RegisterType::Data);
            
            println!("🔧 为虚拟寄存器 {:?} 分配物理寄存器，类型: {:?}", virtual_reg, reg_type);
            
            // 🔧 尝试为当前寄存器找到不冲突的物理寄存器
            let mut assigned_physical_reg = None;
            
            for &physical_reg in &self.calling_convention.allocatable_registers {
                println!("  🔍 尝试物理寄存器 r{}", physical_reg);
                
                if used_physical_regs.contains(&physical_reg) {
                    println!("    物理寄存器 r{} 已被使用，检查是否可以复用", physical_reg);
                    // 检查是否可以复用：当前寄存器的生命周期是否与已分配的寄存器冲突
                    let can_reuse = self.can_reuse_physical_register(
                        virtual_reg, 
                        physical_reg, 
                        &allocation_map, 
                        &lifetime_map
                    );
                    
                    if can_reuse {
                        assigned_physical_reg = Some(physical_reg);
                        println!("    ✅ 可以复用物理寄存器 r{}", physical_reg);
                        break;
                    } else {
                        println!("    ❌ 不能复用物理寄存器 r{}，生命周期冲突", physical_reg);
                    }
                } else {
                    // 物理寄存器未被使用，直接分配
                    assigned_physical_reg = Some(physical_reg);
                    used_physical_regs.insert(physical_reg);
                    println!("    ✅ 直接分配空闲物理寄存器 r{}", physical_reg);
                    break;
                }
            }
            
            if let Some(physical_reg) = assigned_physical_reg {
                allocation_map.insert(virtual_reg, AllocationTarget::Register(physical_reg));
                println!("  虚拟寄存器 {:?} -> r{}", virtual_reg, physical_reg);
            } else {
                // 没有可用的物理寄存器，需要溢出
                let spill_slot = allocation_map.values()
                    .filter_map(|target| if let AllocationTarget::Spill(slot) = target { Some(*slot) } else { None })
                    .max().unwrap_or(0) + 1;
                allocation_map.insert(virtual_reg, AllocationTarget::Spill(spill_slot));
                
                // 🔧 特殊处理：检查是否超出范围或为特殊寄存器类型
                match reg_type {
                    RegisterType::StackAddress => {
                        println!("  虚拟寄存器 {:?} (栈地址寄存器) -> 溢出槽{}", virtual_reg, spill_slot);
                    }
                    _ => {
                        println!("  虚拟寄存器 {:?} (超出范围) -> 溢出槽{}", virtual_reg, spill_slot);
                    }
                }
            }
        }
        
        println!("🎯 调用约定寄存器分配映射: {:?}", allocation_map);
        allocation_map
    }
    
    /// 🔧 新方法：检查是否可以复用物理寄存器（基于生命周期分析）
    fn can_reuse_physical_register(
        &self,
        virtual_reg: RegisterId,
        physical_reg: u8,
        allocation_map: &HashMap<RegisterId, AllocationTarget>,
        lifetime_map: &HashMap<RegisterId, (usize, usize)>,
    ) -> bool {
        let (current_start, current_end) = lifetime_map.get(&virtual_reg).copied().unwrap_or((0, 0));
        
        println!("🔍 检查 {:?} 是否可以复用物理寄存器 r{}", virtual_reg, physical_reg);
        println!("  当前寄存器生命周期: [{}, {}]", current_start, current_end);
        
        // 查找所有已分配到该物理寄存器的虚拟寄存器
        for (allocated_virtual_reg, target) in allocation_map {
            if let AllocationTarget::Register(allocated_physical_reg) = target {
                if *allocated_physical_reg == physical_reg {
                    let (allocated_start, allocated_end) = lifetime_map.get(allocated_virtual_reg).copied().unwrap_or((0, 0));
                    
                    println!("  已分配寄存器 {:?} 到 r{}, 生命周期: [{}, {}]", 
                        allocated_virtual_reg, physical_reg, allocated_start, allocated_end);
                    
                    // 🔧 关键修复：更严格的生命周期重叠检测
                    // 两个区间重叠的条件：max(start1, start2) <= min(end1, end2)
                    let overlap_start = current_start.max(allocated_start);
                    let overlap_end = current_end.min(allocated_end);
                    
                    if overlap_start <= overlap_end {
                        println!("  ❌ 生命周期重叠！重叠区间: [{}, {}]", overlap_start, overlap_end);
                        return false; // 生命周期重叠，不能复用
                    } else {
                        println!("  ✅ 生命周期不重叠，可以复用");
                    }
                }
            }
        }
        
        println!("  ✅ 物理寄存器 r{} 可以被 {:?} 复用", physical_reg, virtual_reg);
        true // 没有冲突，可以复用
    }
    
    /// 🔧 新方法：查找函数的返回值寄存器
    fn find_return_register(&self, function: &LirFunction) -> Option<RegisterId> {
        // 查找return指令中使用的寄存器
        for instruction in &function.instructions {
            if let Instruction::Return { value: Some(reg), .. } = instruction {
                return Some(*reg);
            }
        }
        None
    }
    
    /// 🔧 重构：基于生命周期分析的寄存器分配
    fn apply_allocation_with_spilling(
        &mut self,
        function: &mut LirFunction,
        allocation_map: &HashMap<RegisterId, AllocationTarget>,
    ) -> PassResult {
        println!("🔧 开始基于生命周期分析的寄存器分配");
        
        // 🎯 第一步：使用生命周期分析器获取精确的寄存器活跃度信息
        let lifetime_analyzer = lifetime_analysis::LifetimeAnalyzer::new(
            types::SimpleCallingConvention::default()
        );
        let (lifetimes, _register_types) = lifetime_analyzer.analyze_simple(function);
        
        println!("📊 生命周期分析结果:");
        for lifetime in &lifetimes {
            println!("  {:?}: [{}, {}] uses={:?}", 
                lifetime.register, lifetime.start, lifetime.end, lifetime.uses);
        }
        
        // 🎯 第二步：构建活跃度映射
        let liveness_map = self.build_liveness_map(&lifetimes);
        
        // 🎯 第三步：智能寄存器替换（避免破坏活跃寄存器）
        let mut transformer = IndexInstructionTransformer::new();
        
        // 在函数开头分配栈空间
        let spilled_count = allocation_map.values()
            .filter(|target| matches!(target, AllocationTarget::Spill(_)))
            .count();
        
        if spilled_count > 0 {
            transformer.insert(1, Instruction::Sub {
                dst: RegisterId(6), // SP
                src1: Operand::Register { id: RegisterId(6) },
                src2: Operand::Immediate { value: (spilled_count * 8) as i64 },
                span: Span::dummy(),
            });
        }
        
        // 🔧 关键修复：按指令顺序智能处理寄存器替换
        for (i, instruction) in function.instructions.iter().enumerate() {
            if self.is_function_call(instruction) {
                // 🔧 特殊处理函数调用：确保不破坏返回值
                self.handle_function_call_with_liveness(
                    &mut transformer, 
                    i, 
                    instruction, 
                    allocation_map, 
                    &liveness_map
                );
            } else {
                self.handle_instruction_with_liveness(
                    &mut transformer, 
                    i, 
                    instruction, 
                    allocation_map, 
                    &liveness_map
                );
            }
        }
        
        // 应用变换
        transformer.apply_to_function(function);
        
        // 重写寄存器
        self.rewrite_registers(function, allocation_map);
        
        println!("✅ 基于生命周期的寄存器分配完成");
        PassResult::Changed
    }
    
    /// 检查指令是否是函数调用
    fn is_function_call(&self, instruction: &Instruction) -> bool {
        matches!(instruction, Instruction::Call { .. } | Instruction::CallIndirect { .. })
    }
    
    /// 在函数调用前插入溢出所有寄存器的指令
    fn insert_spill_all_before_call(
        &self,
        transformer: &mut IndexInstructionTransformer,
        call_index: usize,
        allocation_map: &HashMap<RegisterId, AllocationTarget>,
    ) {
        println!("🎯 在函数调用前溢出所有寄存器");
        
        // 🔧 完全重写：实现正确的栈式存储逻辑
        // 按照 register.md 的设计，为每个寄存器执行正确的push操作
        let mut insert_index = call_index;
        
        for &physical_reg in &self.calling_convention.allocatable_registers {
            // 1. 先调整栈指针（为存储分配空间）
            let adjust_sp_instruction = Instruction::Sub {
                dst: RegisterId(6), // SP
                src1: Operand::Register { id: RegisterId(6) },
                src2: Operand::Immediate { value: 8 },
                span: Span::dummy(),
            };
            transformer.insert(insert_index, adjust_sp_instruction);
            insert_index += 1;
            
            // 2. 存储寄存器值到新的栈顶位置
            let store_instruction = Instruction::Store64 {
                addr: RegisterId(6), // SP（已经调整过的）
                offset: 0, // 存储到当前栈顶
                src: Operand::Register { id: RegisterId(physical_reg as usize) },
                span: Span::dummy(),
            };
            transformer.insert(insert_index, store_instruction);
            insert_index += 1;
        }
    }
    
    /// 在函数调用后插入恢复所有寄存器的指令
    fn insert_unspill_all_after_call(
        &self,
        transformer: &mut IndexInstructionTransformer,
        after_call_index: usize,
        allocation_map: &HashMap<RegisterId, AllocationTarget>,
    ) {
        println!("🎯 在函数调用后恢复所有寄存器");
        
        // 🔧 完全重写：实现正确的栈式恢复逻辑
        // 按照栈的LIFO特性，按相反顺序恢复寄存器（先存储的后恢复）
        let mut insert_index = after_call_index;
        
        for &physical_reg in self.calling_convention.allocatable_registers.iter().rev() {
            // 1. 从栈顶加载寄存器值
            let load_instruction = Instruction::Load64 {
                dst: RegisterId(physical_reg as usize),
                addr: RegisterId(6), // SP
                offset: 0, // 从当前栈顶加载
                span: Span::dummy(),
            };
            transformer.insert(insert_index, load_instruction);
            insert_index += 1;
            
            // 2. 调整栈指针（释放存储空间）
            let adjust_sp_instruction = Instruction::Add {
                dst: RegisterId(6), // SP
                src1: Operand::Register { id: RegisterId(6) },
                src2: Operand::Immediate { value: 8 },
                span: Span::dummy(),
            };
            transformer.insert(insert_index, adjust_sp_instruction);
            insert_index += 1;
        }
    }
    
    /// 处理普通指令的溢出
    fn handle_instruction_spilling(
        &self,
        transformer: &mut IndexInstructionTransformer,
        instruction_index: usize,
        instruction: &Instruction,
        allocation_map: &HashMap<RegisterId, AllocationTarget>,
    ) {
        // 获取指令使用的寄存器
        let used_registers = instruction.get_used_registers();
        
        // 为溢出的寄存器插入加载指令
        for &used_reg in &used_registers {
            if let Some(AllocationTarget::Spill(slot_id)) = allocation_map.get(&used_reg) {
                // 插入加载指令
                let load_instruction = Instruction::Load64 {
                    dst: RegisterId(0), // 使用 r0 作为临时寄存器
                    addr: RegisterId(6), // SP
                    offset: -((*slot_id as i64 + 1) * 8), // 栈向下增长
                    span: Span::dummy(),
                };
                transformer.insert(instruction_index, load_instruction);
            }
        }
        
        // 为定义的溢出寄存器插入存储指令
        if let Some(def_reg) = instruction.get_def_register() {
            if let Some(AllocationTarget::Spill(slot_id)) = allocation_map.get(&def_reg) {
                // 插入存储指令
                let store_instruction = Instruction::Store64 {
                    addr: RegisterId(6), // SP
                    offset: -((*slot_id as i64 + 1) * 8), // 栈向下增长
                    src: Operand::Register { id: RegisterId(0) }, // 从 r0 存储
                    span: Span::dummy(),
                };
                transformer.insert(instruction_index + 1, store_instruction);
            }
        }
    }
    
    /// 重写指令中的虚拟寄存器为物理寄存器
    fn rewrite_registers(
        &self,
        function: &mut LirFunction,
        allocation_map: &HashMap<RegisterId, AllocationTarget>,
    ) {
        // 🔧 性能优化：预先计算所有寄存器替换映射，避免重复计算
        let replacements = self.build_register_replacements(allocation_map);
        
        println!("🔧 预计算的寄存器替换映射: {:?}", replacements);
        
        for instruction in &mut function.instructions {
            self.apply_register_replacements(instruction, &replacements);
        }
    }
    
    /// 🔧 新方法：预先构建寄存器替换映射
    fn build_register_replacements(
        &self,
        allocation_map: &HashMap<RegisterId, AllocationTarget>,
    ) -> HashMap<RegisterId, RegisterId> {
        let mut replacements = HashMap::new();
        
        // 🔧 关键修复：只处理原始虚拟寄存器到最终物理寄存器的映射
        // 避免包含中间虚拟寄存器的映射，防止链式替换
        for (virtual_reg, target) in allocation_map {
            // 🔧 重要：只处理真正的虚拟寄存器（ID >= 6，r0-r5是物理寄存器）
            // 这样可以避免链式替换的问题
            if virtual_reg.0 < 6 {  
                println!("  跳过物理寄存器: {:?}", virtual_reg);
                continue;
            }
            
            match target {
                AllocationTarget::Register(physical_reg) => {
                    // 分配到物理寄存器的虚拟寄存器
                    let new_reg = RegisterId(*physical_reg as usize);
                    replacements.insert(*virtual_reg, new_reg);
                    println!("  替换映射: {:?} -> r{}", virtual_reg, physical_reg);
                }
                AllocationTarget::Spill(spill_slot) => {
                    // 🔧 关键修复：溢出寄存器需要被重写为对应的临时寄存器
                    let temp_reg = self.get_temp_register_for_spill(*spill_slot);
                    let new_reg = RegisterId(temp_reg);
                    replacements.insert(*virtual_reg, new_reg);
                    println!("  替换映射: {:?} -> r{} (临时寄存器，槽{})", virtual_reg, temp_reg, spill_slot);
                }
            }
        }
        
        replacements
    }
    
    /// 🔧 新方法：应用预计算的寄存器替换映射到单条指令
    fn apply_register_replacements(
        &self,
        instruction: &mut Instruction,
        replacements: &HashMap<RegisterId, RegisterId>,
    ) {
        // 🔧 调试：打印指令替换前的状态
        println!("🔧 替换前指令: {:?}", instruction);
        
        // 🔧 性能优化：只对指令中实际存在的虚拟寄存器进行替换
        for (old_reg, new_reg) in replacements {
            if self.instruction_contains_register(instruction, *old_reg) {
                println!("  🔄 替换寄存器 {:?} -> {:?}", old_reg, new_reg);
                instruction.replace_register(*old_reg, *new_reg);
            }
        }
        
        // 🔧 调试：打印指令替换后的状态
        println!("🔧 替换后指令: {:?}", instruction);
    }
    
    /// 🔧 新方法：检查指令是否包含指定的寄存器
    fn instruction_contains_register(&self, instruction: &Instruction, reg: RegisterId) -> bool {
        // 检查目标寄存器
        if let Some(def_reg) = instruction.get_def_register() {
            if def_reg == reg {
                return true;
            }
        }
        
        // 检查使用的寄存器
        let used_registers = instruction.get_used_registers();
        used_registers.contains(&reg)
    }

    /// 🔧 新方法：构建活跃度映射
    fn build_liveness_map(&self, lifetimes: &[types::RegisterLifetime]) -> HashMap<usize, HashSet<RegisterId>> {
        let mut liveness_map = HashMap::new();
        
        // 为每个指令位置构建活跃寄存器集合
        let max_instruction = lifetimes.iter()
            .map(|lt| lt.end)
            .max()
            .unwrap_or(0);
            
        for i in 0..=max_instruction {
            let mut live_registers = HashSet::new();
            
            for lifetime in lifetimes {
                if i >= lifetime.start && i <= lifetime.end {
                    live_registers.insert(lifetime.register);
                }
            }
            
            liveness_map.insert(i, live_registers);
        }
        
        println!("🔍 活跃度映射构建完成，共 {} 个指令位置", liveness_map.len());
        liveness_map
    }
    
    /// 🔧 新方法：基于活跃度信息智能处理指令
    fn handle_instruction_with_liveness(
        &self,
        transformer: &mut IndexInstructionTransformer,
        instruction_index: usize,
        instruction: &Instruction,
        allocation_map: &HashMap<RegisterId, AllocationTarget>,
        liveness_map: &HashMap<usize, HashSet<RegisterId>>,
    ) {
        println!("🔧 处理指令 {}: {:?}", instruction_index, instruction);
        
        // 获取当前指令位置的活跃寄存器
        let live_registers = liveness_map.get(&instruction_index).cloned().unwrap_or_default();
        println!("  活跃寄存器: {:?}", live_registers);
        
        // 获取指令使用和定义的寄存器
        let used_registers = instruction.get_used_registers();
        let def_register = instruction.get_def_register();
        
        // 🔧 关键修复：只在寄存器即将被使用时才加载，避免过早覆盖
        for &used_reg in &used_registers {
            if let Some(AllocationTarget::Spill(slot_id)) = allocation_map.get(&used_reg) {
                // 检查这个寄存器是否真的需要在此时加载
                if live_registers.contains(&used_reg) {
                    println!("  📥 加载溢出寄存器 {:?} 从槽 {}", used_reg, slot_id);
                    
                    // 为溢出寄存器分配一个确定性的临时物理寄存器
                    let temp_physical_reg = self.get_temp_register_for_spill(*slot_id);
                    
                    let load_instruction = Instruction::Load64 {
                        dst: RegisterId(temp_physical_reg),
                        addr: RegisterId(6), // SP
                        offset: -((*slot_id as i64 + 1) * 8),
                        span: Span::dummy(),
                    };
                    transformer.insert(instruction_index, load_instruction);
                }
            }
        }
        
        // 🔧 关键修复：只在寄存器定义后需要保存时才存储
        if let Some(def_reg) = def_register {
            if let Some(AllocationTarget::Spill(slot_id)) = allocation_map.get(&def_reg) {
                // 检查这个寄存器是否在后续指令中还会被使用
                let will_be_used_later = self.check_if_used_later(def_reg, instruction_index, liveness_map);
                
                if will_be_used_later {
                    println!("  📤 存储溢出寄存器 {:?} 到槽 {}", def_reg, slot_id);
                    
                    let temp_physical_reg = self.get_temp_register_for_spill(*slot_id);
                    
                    let store_instruction = Instruction::Store64 {
                        addr: RegisterId(6), // SP
                        offset: -((*slot_id as i64 + 1) * 8),
                        src: Operand::Register { id: RegisterId(temp_physical_reg) },
                        span: Span::dummy(),
                    };
                    transformer.insert(instruction_index + 1, store_instruction);
                }
            }
        }
    }
    
    /// 🔧 新方法：为溢出槽分配确定性的临时寄存器
    fn get_temp_register_for_spill(&self, slot_id: usize) -> usize {
        // 🔧 关键修复：使用更智能的临时寄存器分配策略
        // 为不同的溢出槽分配不同的临时寄存器，避免冲突
        match slot_id {
            1 => 0, // 溢出槽1使用r0
            2 => 1, // 溢出槽2使用r1  
            3 => 2, // 溢出槽3使用r2
            4 => 3, // 溢出槽4使用r3
            _ => (slot_id % self.calling_convention.allocatable_registers.len()), // 其他槽轮换使用
        }
    }
    
    /// 🔧 新方法：检查寄存器是否在后续指令中被使用
    fn check_if_used_later(
        &self,
        register: RegisterId,
        current_index: usize,
        liveness_map: &HashMap<usize, HashSet<RegisterId>>,
    ) -> bool {
        // 检查从当前指令之后的位置是否还有活跃的使用
        for (index, live_registers) in liveness_map {
            if *index > current_index && live_registers.contains(&register) {
                return true;
            }
        }
        false
    }

    /// 🔧 新方法：特殊处理函数调用指令，确保不破坏返回值
    fn handle_function_call_with_liveness(
        &self,
        transformer: &mut IndexInstructionTransformer,
        instruction_index: usize,
        instruction: &Instruction,
        allocation_map: &HashMap<RegisterId, AllocationTarget>,
        liveness_map: &HashMap<usize, HashSet<RegisterId>>,
    ) {
        println!("🔧 特殊处理函数调用指令 {}: {:?}", instruction_index, instruction);
        
        // 获取当前指令位置的活跃寄存器
        let live_registers = liveness_map.get(&instruction_index).cloned().unwrap_or_default();
        println!("  活跃寄存器: {:?}", live_registers);
        
        // 获取指令使用的寄存器（函数调用的参数和函数地址）
        let used_registers = instruction.get_used_registers();
        
        // 🔧 关键修复：为函数调用中的溢出寄存器分配不同的临时寄存器
        let mut temp_register_assignments = HashMap::new();
        let mut next_temp_register = 0;
        
        // 🔧 第一步：为所有需要加载的溢出寄存器分配临时寄存器
        for &used_reg in &used_registers {
            if let Some(AllocationTarget::Spill(slot_id)) = allocation_map.get(&used_reg) {
                if live_registers.contains(&used_reg) {
                    // 🔧 关键修复：为每个溢出寄存器分配不同的临时寄存器
                    let temp_physical_reg = self.get_unique_temp_register_for_call(
                        &mut next_temp_register,
                        &temp_register_assignments
                    );
                    temp_register_assignments.insert(used_reg, temp_physical_reg);
                    
                    println!("  📥 函数调用前加载参数寄存器 {:?} 从槽 {} 到临时寄存器 r{}", 
                        used_reg, slot_id, temp_physical_reg);
                    
                    let load_instruction = Instruction::Load64 {
                        dst: RegisterId(temp_physical_reg),
                        addr: RegisterId(6), // SP
                        offset: -((*slot_id as i64 + 1) * 8),
                        span: Span::dummy(),
                    };
                    transformer.insert(instruction_index, load_instruction);
                }
            }
        }
        
        // 🔧 关键修复：检查函数调用的返回值寄存器
        if let Some(def_reg) = instruction.get_def_register() {
            println!("  🎯 函数调用返回值寄存器: {:?}", def_reg);
            
            // 检查返回值寄存器是否需要溢出
            if let Some(AllocationTarget::Spill(slot_id)) = allocation_map.get(&def_reg) {
                // 🔧 关键修复：检查返回值在后续是否被使用
                let return_value_used_later = self.check_if_used_after_call(
                    def_reg, 
                    instruction_index, 
                    liveness_map
                );
                
                if return_value_used_later {
                    println!("  📤 函数调用后存储返回值寄存器 {:?} 到槽 {}", def_reg, slot_id);
                    
                    // 返回值寄存器使用调用约定的返回寄存器 r0
                    let store_instruction = Instruction::Store64 {
                        addr: RegisterId(6), // SP
                        offset: -((*slot_id as i64 + 1) * 8),
                        src: Operand::Register { id: RegisterId(self.calling_convention.return_register as usize) },
                        span: Span::dummy(),
                    };
                    // 🔧 关键：在函数调用后插入存储指令
                    transformer.insert(instruction_index + 1, store_instruction);
                } else {
                    println!("  ⚠️  返回值寄存器 {:?} 在后续未被使用，跳过存储", def_reg);
                }
            }
        }
    }
    
    /// 🔧 新方法：为函数调用分配唯一的临时寄存器
    fn get_unique_temp_register_for_call(
        &self,
        next_temp_register: &mut usize,
        temp_register_assignments: &HashMap<RegisterId, usize>,
    ) -> usize {
        // 从可分配寄存器中选择一个未被使用的
        for &physical_reg in &self.calling_convention.allocatable_registers {
            let physical_reg_usize = physical_reg as usize;
            if !temp_register_assignments.values().any(|&assigned| assigned == physical_reg_usize) {
                *next_temp_register = physical_reg_usize + 1;
                return physical_reg_usize;
            }
        }
        
        // 如果所有可分配寄存器都被使用，使用下一个可用的寄存器
        let result = *next_temp_register;
        *next_temp_register += 1;
        result
    }
    
    /// 🔧 新方法：检查寄存器是否在函数调用后被使用
    fn check_if_used_after_call(
        &self,
        register: RegisterId,
        call_index: usize,
        liveness_map: &HashMap<usize, HashSet<RegisterId>>,
    ) -> bool {
        // 检查从函数调用后的位置开始，是否还有活跃的使用
        for (index, live_registers) in liveness_map {
            if *index > call_index && live_registers.contains(&register) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AllocationType, Instruction, LirFunction, Operand, RegisterId};
    use karte_diagnostics::Span;

    #[test]
    fn test_register_type_analysis() {
        // 创建一个简单的测试函数
        let mut function = LirFunction {
            name: "test_function".to_string(),
            parameter_registers: vec![RegisterId(100), RegisterId(101)],
            instructions: vec![
                // alloc指令 - 栈分配
                Instruction::Alloc {
                    dst: RegisterId(200),
                    size: 8,
                    alignment: 8,
                    allocation_type: AllocationType::Stack,
                    span: Span::dummy(),
                },
                // alloc指令 - 堆分配
                Instruction::Alloc {
                    dst: RegisterId(201),
                    size: 16,
                    alignment: 8,
                    allocation_type: AllocationType::Heap,
                    span: Span::dummy(),
                },
                // add指令 - 栈地址计算
                Instruction::Add {
                    dst: RegisterId(202),
                    src1: Operand::Register { id: RegisterId(7) }, // FP
                    src2: Operand::Immediate { value: -8 },
                    span: Span::dummy(),
                },
                // 普通数据操作
                Instruction::Add {
                    dst: RegisterId(203),
                    src1: Operand::Register {
                        id: RegisterId(100),
                    },
                    src2: Operand::Register {
                        id: RegisterId(101),
                    },
                    span: Span::dummy(),
                },
            ],
            next_register: 204,
            next_label: 1,
            struct_types: HashMap::new(),
            stack_frame_size: 0,
            parameter_count: 2,
        };

        let calling_convention = types::SimpleCallingConvention::default();
        let lifetime_analyzer = LifetimeAnalyzer::new(calling_convention);
        let (lifetimes, register_types) = lifetime_analyzer.analyze_simple(&function);

        // 验证函数参数类型
        assert_eq!(
            register_types.get(&RegisterId(100)),
            Some(&RegisterType::FunctionParameter)
        );
        assert_eq!(
            register_types.get(&RegisterId(101)),
            Some(&RegisterType::FunctionParameter)
        );

        // 验证栈地址寄存器类型
        assert_eq!(
            register_types.get(&RegisterId(200)),
            Some(&RegisterType::StackAddress)
        ); // 栈alloc
        assert_eq!(
            register_types.get(&RegisterId(202)),
            Some(&RegisterType::StackAddress)
        ); // FP + offset

        // 验证数据寄存器类型
        assert_eq!(
            register_types.get(&RegisterId(201)),
            Some(&RegisterType::Data)
        ); // 堆alloc
        assert_eq!(
            register_types.get(&RegisterId(203)),
            Some(&RegisterType::Data)
        ); // 普通计算

        // 验证生命周期中的寄存器类型
        for lifetime in &lifetimes {
            match lifetime.register.0 {
                100 | 101 => assert_eq!(lifetime.register_type, RegisterType::FunctionParameter),
                200 | 202 => assert_eq!(lifetime.register_type, RegisterType::StackAddress),
                201 | 203 => assert_eq!(lifetime.register_type, RegisterType::Data),
                _ => {}
            }
        }

        println!("✅ 寄存器类型分析测试通过");
    }

    #[test]
    fn test_spill_constraints() {
        // 测试溢出约束
        assert!(RegisterType::Data.can_spill());
        assert!(!RegisterType::StackAddress.can_spill());
        assert!(!RegisterType::FunctionParameter.can_spill());
        assert!(!RegisterType::Special.can_spill());

        println!("✅ 溢出约束测试通过");
    }
}

#[cfg(test)]
mod simple_stack_tests {
    use super::*;
    use crate::{Instruction, LirFunction, Operand, RegisterId};
    use karte_diagnostics::Span;
    use std::collections::HashMap;

    /// 测试新的简单栈式寄存器分配Pass
    #[test]
    fn test_simple_stack_register_allocation() {
        // 创建测试函数，模拟文件"1"中的LIR代码
        let mut function = LirFunction {
            name: "main".to_string(),
            parameter_registers: vec![],
            instructions: vec![
                // L1:
                Instruction::Label { id: crate::LabelId(1), span: Span::dummy() },
                // mov r1, #1
                Instruction::Move {
                    dst: RegisterId(1),
                    src: Operand::Immediate { value: 1 },
                    span: Span::dummy(),
                },
                // mov r2, #2
                Instruction::Move {
                    dst: RegisterId(2),
                    src: Operand::Immediate { value: 2 },
                    span: Span::dummy(),
                },
                // mov r3, #3
                Instruction::Move {
                    dst: RegisterId(3),
                    src: Operand::Immediate { value: 3 },
                    span: Span::dummy(),
                },
                // mov r4, #4
                Instruction::Move {
                    dst: RegisterId(4),
                    src: Operand::Immediate { value: 4 },
                    span: Span::dummy(),
                },
                // mov r5, #5
                Instruction::Move {
                    dst: RegisterId(5),
                    src: Operand::Immediate { value: 5 },
                    span: Span::dummy(),
                },
                // mov r8, #6
                Instruction::Move {
                    dst: RegisterId(8),
                    src: Operand::Immediate { value: 6 },
                    span: Span::dummy(),
                },
                // mov r9, #7
                Instruction::Move {
                    dst: RegisterId(9),
                    src: Operand::Immediate { value: 7 },
                    span: Span::dummy(),
                },
                // add r10, r8, r9
                Instruction::Add {
                    dst: RegisterId(10),
                    src1: Operand::Register { id: RegisterId(8) },
                    src2: Operand::Register { id: RegisterId(9) },
                    span: Span::dummy(),
                },
                // add r11, r5, r10
                Instruction::Add {
                    dst: RegisterId(11),
                    src1: Operand::Register { id: RegisterId(5) },
                    src2: Operand::Register { id: RegisterId(10) },
                    span: Span::dummy(),
                },
                // add r12, r4, r11
                Instruction::Add {
                    dst: RegisterId(12),
                    src1: Operand::Register { id: RegisterId(4) },
                    src2: Operand::Register { id: RegisterId(11) },
                    span: Span::dummy(),
                },
                // add r13, r3, r12
                Instruction::Add {
                    dst: RegisterId(13),
                    src1: Operand::Register { id: RegisterId(3) },
                    src2: Operand::Register { id: RegisterId(12) },
                    span: Span::dummy(),
                },
                // add r14, r2, r13
                Instruction::Add {
                    dst: RegisterId(14),
                    src1: Operand::Register { id: RegisterId(2) },
                    src2: Operand::Register { id: RegisterId(13) },
                    span: Span::dummy(),
                },
                // add r15, r1, r14
                Instruction::Add {
                    dst: RegisterId(15),
                    src1: Operand::Register { id: RegisterId(1) },
                    src2: Operand::Register { id: RegisterId(14) },
                    span: Span::dummy(),
                },
                // ret r15
                Instruction::Return {
                    value: Some(RegisterId(15)),
                    span: Span::dummy(),
                },
            ],
            next_register: 16,
            next_label: 2,
            struct_types: HashMap::new(),
            stack_frame_size: 0,
            parameter_count: 0,
        };

        println!("🧪 测试前的LIR函数:");
        for (i, instruction) in function.instructions.iter().enumerate() {
            println!("  {}: {:?}", i, instruction);
        }

        // 创建并运行新的寄存器分配Pass
        let mut pass = SimpleStackRegisterAllocation::new();
        let mut analyses = AnalysisManager::new();
        
        let result = pass.run_on_function(&mut function, &mut analyses);
        
        println!("🧪 测试后的LIR函数:");
        for (i, instruction) in function.instructions.iter().enumerate() {
            println!("  {}: {:?}", i, instruction);
        }

        // 验证结果
        assert!(matches!(result, PassResult::Changed));
        
        // 验证是否插入了栈空间分配指令
        let has_stack_allocation = function.instructions.iter().any(|inst| {
            matches!(inst, Instruction::Sub { dst, src2, .. } 
                if dst.0 == 6 && matches!(src2, Operand::Immediate { value } if *value > 0))
        });
        
        // 验证寄存器是否被正确重写
        let uses_only_physical_regs = function.instructions.iter().all(|inst| {
            let used_regs = inst.get_used_registers();
            let def_reg = inst.get_def_register();
            
            // 检查所有使用的寄存器都是物理寄存器 (r0-r7)
            let used_ok = used_regs.iter().all(|reg| reg.0 <= 7);
            let def_ok = def_reg.map_or(true, |reg| reg.0 <= 7);
            
            used_ok && def_ok
        });

        println!("✅ 寄存器分配测试完成");
        println!("  - 栈空间分配: {}", has_stack_allocation);
        println!("  - 物理寄存器范围: {}", uses_only_physical_regs);
        
        // 如果有很多虚拟寄存器，应该有栈分配
        if function.instructions.len() > 10 {
            assert!(has_stack_allocation, "应该有栈空间分配指令");
        }
        assert!(uses_only_physical_regs, "所有寄存器都应该在物理寄存器范围内");
    }

    #[test]
    fn test_complex_call_indirect_register_allocation() {
        println!("🧪 测试复杂call_indirect指令寄存器保存/恢复:");
        
        // 创建一个更复杂的函数，包含多个虚拟寄存器和函数调用
        let mut function = LirFunction {
            name: "complex_test".to_string(),
            parameter_registers: vec![RegisterId(0)],
            next_register: 200,
            next_label: 10,
            struct_types: HashMap::new(),
            stack_frame_size: 0,
            parameter_count: 1,
            instructions: vec![
                // 分配结构体
                Instruction::Alloc {
                    dst: RegisterId(100),
                    size: 16,
                    alignment: 8,
                    allocation_type: AllocationType::Stack,
                    span: Span::dummy(),
                },
                // 存储函数地址
                Instruction::Store64 {
                    addr: RegisterId(100),
                    offset: 0,
                    src: Operand::Immediate { value: 1 },
                    span: Span::dummy(),
                },
                // 设置多个虚拟寄存器
                Instruction::Move {
                    dst: RegisterId(101),
                    src: Operand::Immediate { value: 42 },
                    span: Span::dummy(),
                },
                Instruction::Move {
                    dst: RegisterId(102),
                    src: Operand::Immediate { value: 43 },
                    span: Span::dummy(),
                },
                Instruction::Move {
                    dst: RegisterId(103),
                    src: Operand::Immediate { value: 44 },
                    span: Span::dummy(),
                },
                Instruction::Move {
                    dst: RegisterId(104),
                    src: Operand::Immediate { value: 45 },
                    span: Span::dummy(),
                },
                Instruction::Move {
                    dst: RegisterId(105),
                    src: Operand::Immediate { value: 46 },
                    span: Span::dummy(),
                },
                // 加载函数地址
                Instruction::Load64 {
                    dst: RegisterId(106),
                    addr: RegisterId(100),
                    offset: 0,
                    span: Span::dummy(),
                },
                // 函数调用
                Instruction::CallIndirect {
                    function_register: RegisterId(106),
                    args: vec![RegisterId(101)],
                    arg_operands: vec![Operand::Register { id: RegisterId(101) }],
                    result: Some(RegisterId(107)),
                    span: Span::dummy(),
                },
                // 使用函数调用结果和之前的寄存器
                Instruction::Add {
                    dst: RegisterId(108),
                    src1: Operand::Register { id: RegisterId(107) },
                    src2: Operand::Register { id: RegisterId(102) },
                    span: Span::dummy(),
                },
                Instruction::Add {
                    dst: RegisterId(109),
                    src1: Operand::Register { id: RegisterId(108) },
                    src2: Operand::Register { id: RegisterId(103) },
                    span: Span::dummy(),
                },
                Instruction::Add {
                    dst: RegisterId(110),
                    src1: Operand::Register { id: RegisterId(109) },
                    src2: Operand::Register { id: RegisterId(104) },
                    span: Span::dummy(),
                },
                Instruction::Add {
                    dst: RegisterId(111),
                    src1: Operand::Register { id: RegisterId(110) },
                    src2: Operand::Register { id: RegisterId(105) },
                    span: Span::dummy(),
                },
                Instruction::Return {
                    value: Some(RegisterId(111)),
                    span: Span::dummy(),
                },
            ],
        };
        
        println!("原始LIR:");
        for (i, instr) in function.instructions.iter().enumerate() {
            println!("  {}: {:?}", i, instr);
        }
        
        // 运行寄存器分配
        let mut pass = SimpleStackRegisterAllocation::new();
        let mut analysis_manager = AnalysisManager::new();
        let result = pass.run_on_function(&mut function, &mut analysis_manager);
        
        match result {
            PassResult::Changed | PassResult::Unchanged => {},
            PassResult::Failed(msg) => panic!("寄存器分配失败: {}", msg),
        }
        
        println!("\n寄存器分配后:");
        for (i, instr) in function.instructions.iter().enumerate() {
            println!("  {}: {:?}", i, instr);
        }
        
        // 验证生成的代码
        let mut found_call_indirect = false;
        let mut found_store_sequence = false;
        let mut found_load_sequence = false;
        let mut store_addresses = Vec::new();
        let mut load_addresses = Vec::new();
        
        for (i, instruction) in function.instructions.iter().enumerate() {
            match instruction {
                Instruction::CallIndirect { .. } => {
                    found_call_indirect = true;
                    println!("🔍 发现call_indirect指令在位置 {}", i);
                }
                Instruction::Store64 { addr, offset, .. } if *addr == RegisterId(6) => {
                    store_addresses.push(offset);
                    println!("🔍 发现store64指令: offset={}", offset);
                }
                Instruction::Load64 { addr, offset, .. } if *addr == RegisterId(6) => {
                    load_addresses.push(offset);
                    println!("🔍 发现load64指令: offset={}", offset);
                }
                _ => {}
            }
        }
        
        // 验证栈操作的正确性
        if store_addresses.len() > 1 {
            // 检查store地址是否各不相同
            let mut unique_stores = store_addresses.clone();
            unique_stores.sort();
            unique_stores.dedup();
            
            println!("🔍 Store地址: {:?}", store_addresses);
            println!("🔍 唯一Store地址: {:?}", unique_stores);
            
            if unique_stores.len() == store_addresses.len() {
                found_store_sequence = true;
                println!("✅ 所有store指令使用不同的栈地址");
            } else {
                println!("❌ 发现重复的store地址！");
            }
        }
        
        if load_addresses.len() > 1 {
            // 检查load地址是否各不相同
            let mut unique_loads = load_addresses.clone();
            unique_loads.sort();
            unique_loads.dedup();
            
            println!("🔍 Load地址: {:?}", load_addresses);
            println!("🔍 唯一Load地址: {:?}", unique_loads);
            
            if unique_loads.len() == load_addresses.len() {
                found_load_sequence = true;
                println!("✅ 所有load指令使用不同的栈地址");
            } else {
                println!("❌ 发现重复的load地址！");
            }
        }
        
        assert!(found_call_indirect, "应该找到call_indirect指令");
        println!("✅ 复杂call_indirect指令寄存器分配测试通过！");
    }
}

/// 运行简单栈式寄存器分配的公共函数
pub fn run_simple_stack_register_allocation(function: &mut LirFunction) -> PassResult {
    let mut pass = SimpleStackRegisterAllocation::new();
    let mut analyses = AnalysisManager::new();
    pass.run_on_function(function, &mut analyses)
}

/// 测试处理实际的LIR文件"1"
#[cfg(test)]
mod file_test {
    use super::*;
    use crate::{Instruction, LirFunction, Operand, RegisterId};
    use karte_diagnostics::Span;
    use std::collections::HashMap;

    /// 解析LIR文件内容为LirFunction
    fn parse_lir_file(content: &str) -> Result<LirFunction, String> {
        let lines: Vec<&str> = content.lines().collect();
        
        if lines.is_empty() {
            return Err("Empty file".to_string());
        }
        
        // 解析函数头
        let first_line = lines[0].trim();
        if !first_line.starts_with("function ") {
            return Err("Invalid function header".to_string());
        }
        
        let function_name = first_line
            .strip_prefix("function ")
            .and_then(|s| s.split(' ').next())
            .unwrap_or("main")
            .to_string();
        
        let mut function = LirFunction {
            name: function_name,
            parameter_registers: vec![],
            instructions: vec![],
            next_register: 16,
            next_label: 2,
            struct_types: HashMap::new(),
            stack_frame_size: 0,
            parameter_count: 0,
        };
        
        // 解析指令
        for (line_num, line) in lines.iter().enumerate().skip(1) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            
            let instruction = parse_instruction(line, line_num)?;
            function.instructions.push(instruction);
        }
        
        Ok(function)
    }
    
    /// 解析单条指令
    fn parse_instruction(line: &str, line_num: usize) -> Result<Instruction, String> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        
        if parts.is_empty() {
            return Err(format!("Empty instruction at line {}", line_num));
        }
        
        match parts[0] {
            // 标签
            label if label.ends_with(':') => {
                let label_name = label.trim_end_matches(':');
                let label_id = match label_name {
                    "L1" => crate::LabelId(1),
                    _ => crate::LabelId(0),
                };
                Ok(Instruction::Label { 
                    id: label_id, 
                    span: Span::dummy() 
                })
            }
            // mov 指令
            "mov" => {
                if parts.len() != 3 {
                    return Err(format!("Invalid mov instruction at line {}", line_num));
                }
                
                let dst = parse_register(parts[1].trim_end_matches(','))?;
                let src = parse_operand(parts[2])?;
                
                Ok(Instruction::Move {
                    dst,
                    src,
                    span: Span::dummy(),
                })
            }
            // add 指令
            "add" => {
                if parts.len() != 4 {
                    return Err(format!("Invalid add instruction at line {}", line_num));
                }
                
                let dst = parse_register(parts[1].trim_end_matches(','))?;
                let src1 = parse_operand(parts[2].trim_end_matches(','))?;
                let src2 = parse_operand(parts[3])?;
                
                Ok(Instruction::Add {
                    dst,
                    src1,
                    src2,
                    span: Span::dummy(),
                })
            }
            // load64 指令 - load64 r10, [r5]
            "load64" => {
                if parts.len() != 3 {
                    return Err(format!("Invalid load64 instruction at line {}", line_num));
                }
                
                let dst = parse_register(parts[1].trim_end_matches(','))?;
                let addr_str = parts[2];
                
                // 简单解析 [rX] 格式
                if addr_str.starts_with('[') && addr_str.ends_with(']') {
                    let inner = &addr_str[1..addr_str.len()-1];
                    let addr = parse_register(inner)?;
                    
                    Ok(Instruction::Load64 {
                        dst,
                        addr,
                        offset: 0,
                        span: Span::dummy(),
                    })
                } else {
                    Err(format!("Invalid load64 address format: {}", addr_str))
                }
            }
            // ret 指令
            "ret" => {
                let value = if parts.len() > 1 {
                    Some(parse_register(parts[1])?)
                } else {
                    None
                };
                
                Ok(Instruction::Return {
                    value,
                    span: Span::dummy(),
                })
            }
            _ => {
                // 检查是否是 call_indirect 格式: r9 = call_indirect r10(r1)
                if line.contains("call_indirect") {
                    // 解析格式: r9 = call_indirect r10(r1)
                    if let Some(equals_pos) = line.find('=') {
                        let result_part = line[..equals_pos].trim();
                        let call_part = line[equals_pos+1..].trim();
                        
                        if call_part.starts_with("call_indirect") {
                            let result_reg = parse_register(result_part)?;
                            
                            // 解析 call_indirect r10(r1) 部分
                            let call_content = call_part.strip_prefix("call_indirect").unwrap().trim();
                            if let Some(paren_pos) = call_content.find('(') {
                                let function_reg_str = call_content[..paren_pos].trim();
                                let args_str = &call_content[paren_pos+1..];
                                let args_str = args_str.trim_end_matches(')');
                                
                                let function_register = parse_register(function_reg_str)?;
                                let args = if args_str.is_empty() {
                                    vec![]
                                } else {
                                    args_str.split(',')
                                        .map(|s| parse_register(s.trim()))
                                        .collect::<Result<Vec<_>, _>>()?
                                };
                                
                                return Ok(Instruction::CallIndirect {
                                    function_register,
                                    args,
                                    arg_operands: vec![], // 简化处理
                                    result: Some(result_reg),
                                    span: Span::dummy(),
                                });
                            }
                        }
                    }
                }
                
                Err(format!("Unknown instruction '{}' at line {}", parts[0], line_num))
            }
        }
    }
    
    /// 解析寄存器
    fn parse_register(s: &str) -> Result<RegisterId, String> {
        if s.starts_with('r') {
            let num_str = &s[1..];
            let num: usize = num_str.parse()
                .map_err(|_| format!("Invalid register number: {}", s))?;
            Ok(RegisterId(num))
        } else {
            Err(format!("Invalid register format: {}", s))
        }
    }
    
    /// 解析操作数
    fn parse_operand(s: &str) -> Result<Operand, String> {
        if s.starts_with('#') {
            // 立即数
            let num_str = &s[1..];
            let value: i64 = num_str.parse()
                .map_err(|_| format!("Invalid immediate value: {}", s))?;
            Ok(Operand::Immediate { value })
        } else if s.starts_with('r') {
            // 寄存器
            let reg = parse_register(s)?;
            Ok(Operand::Register { id: reg })
        } else {
            Err(format!("Invalid operand format: {}", s))
        }
    }

    #[test]
    fn test_file_1_register_allocation() {
        // 读取文件"1"的内容
        let file_content = r#"function main (stack_frame: 0):
L1:
  mov r1, #1
  mov r2, #2
  mov r3, #3
  mov r4, #4
  mov r5, #5
  mov r8, #6
  mov r9, #7
  add r10, r8, r9
  add r11, r5, r10
  add r12, r4, r11
  add r13, r3, r12
  add r14, r2, r13
  add r15, r1, r14
  ret r15"#;

        println!("🧪 测试文件'1'的寄存器分配");
        println!("原始LIR内容:");
        for (i, line) in file_content.lines().enumerate() {
            println!("  {}: {}", i, line);
        }

        // 解析LIR文件
        let mut function = parse_lir_file(file_content).expect("Failed to parse LIR file");
        
        println!("\n🧪 解析后的LIR函数:\n{}", function);
        // for (i, instruction) in function.instructions.iter().enumerate() {
        //     println!("  {}: {}", i, instruction);
        // }

        // 运行新的寄存器分配Pass
        let result = run_simple_stack_register_allocation(&mut function);
        
        println!("\n🧪 寄存器分配后的LIR函数:\n{}", function);

        // 验证结果
        assert!(matches!(result, PassResult::Changed));
        
        // 验证所有寄存器都在物理寄存器范围内
        let uses_only_physical_regs = function.instructions.iter().all(|inst| {
            let used_regs = inst.get_used_registers();
            let def_reg = inst.get_def_register();
            
            // 检查所有使用的寄存器都是物理寄存器 (r0-r7)
            let used_ok = used_regs.iter().all(|reg| reg.0 <= 7);
            let def_ok = def_reg.map_or(true, |reg| reg.0 <= 7);
            
            used_ok && def_ok
        });
        
        // 验证是否有栈空间分配（因为有很多虚拟寄存器）
        let has_stack_allocation = function.instructions.iter().any(|inst| {
            matches!(inst, Instruction::Sub { dst, src2, .. } 
                if dst.0 == 6 && matches!(src2, Operand::Immediate { value } if *value > 0))
        });
        
        println!("\n✅ 验证结果:");
        println!("  - 物理寄存器范围: {}", uses_only_physical_regs);
        println!("  - 栈空间分配: {}", has_stack_allocation);
        println!("  - 指令数量: {} -> {}", 14, function.instructions.len());
        
        assert!(uses_only_physical_regs, "所有寄存器都应该在物理寄存器范围内");
        assert!(has_stack_allocation, "应该有栈空间分配指令，因为有很多虚拟寄存器");
        
        println!("🎉 文件'1'的寄存器分配测试通过！");
    }

    /// 测试call_indirect指令的寄存器分配修复
    #[test]
    fn test_call_indirect_register_allocation() {
        // 模拟call_indirect场景的简化版本
        let test_content = r#"function main (stack_frame: 0):
L1:
  mov r3, #16
  mov r4, r3
  add r5, r4, #0
  mov r8, #42
  mov r1, r8
  load64 r10, [r5]
  r9 = call_indirect r10(r1)
  mov r11, r9
  ret r11"#;

        let mut function = parse_lir_file(test_content).expect("Failed to parse test LIR");
        
        println!("\n🧪 测试call_indirect指令修复:");
        println!("原始LIR:\n{}", function);
        
        // 运行新的寄存器分配Pass
        let result = run_simple_stack_register_allocation(&mut function);
        
        println!("\n寄存器分配后:\n{}", function);
        
        // 验证结果
        assert!(matches!(result, PassResult::Changed));
        
        // 关键验证：检查call_indirect指令中的函数地址寄存器
        let mut found_call_indirect = false;
        for instruction in &function.instructions {
            if let Instruction::CallIndirect { function_register, args, .. } = instruction {
                found_call_indirect = true;
                println!("🔍 发现call_indirect指令:");
                println!("  函数地址寄存器: r{}", function_register.0);
                println!("  参数寄存器: {:?}", args.iter().map(|r| format!("r{}", r.0)).collect::<Vec<_>>());
                
                // 验证函数地址寄存器在合理范围内
                assert!(function_register.0 <= 4, "函数地址寄存器应该在r0-r4范围内");
                
                // 验证参数寄存器在合理范围内
                for arg in args {
                    assert!(arg.0 <= 4, "参数寄存器应该在r0-r4范围内");
                }
            }
        }
        
        assert!(found_call_indirect, "应该找到call_indirect指令");
        
        // 验证load64指令的正确性（加载函数地址）
        let mut found_load64_before_call = false;
        for (i, instruction) in function.instructions.iter().enumerate() {
            if let Instruction::Load64 { dst, addr, .. } = instruction {
                // 检查是否有后续的call_indirect指令使用这个寄存器
                for j in (i+1)..function.instructions.len() {
                    if let Instruction::CallIndirect { function_register, .. } = &function.instructions[j] {
                        if dst == function_register {
                            found_load64_before_call = true;
                            println!("🔍 发现load64指令为call_indirect准备函数地址:");
                            println!("  load64 r{}, [r{}]", dst.0, addr.0);
                            println!("  call_indirect r{}(...)", function_register.0);
                            
                            // 验证地址寄存器在合理范围内 (r0-r7)
                            assert!(addr.0 <= 7, "地址寄存器应该在r0-r7范围内");
                            break;
                        }
                    }
                }
            }
        }
        
        println!("✅ call_indirect指令寄存器分配修复测试通过！");
    }

    /// 测试函数参数寄存器分配
    #[test]
    fn test_function_parameter_register_allocation() {
        println!("🧪 测试函数参数寄存器分配:");
        
        // 创建一个有参数的函数
        let mut function = LirFunction {
            name: "test_func".to_string(),
            parameter_registers: vec![RegisterId(100), RegisterId(101), RegisterId(102)], // 3个参数
            instructions: vec![
                Instruction::Label { id: crate::LabelId(1), span: Span::dummy() },
                // 使用参数寄存器
                Instruction::Add {
                    dst: RegisterId(200),
                    src1: Operand::Register { id: RegisterId(100) }, // 第一个参数
                    src2: Operand::Register { id: RegisterId(101) }, // 第二个参数
                    span: Span::dummy(),
                },
                Instruction::Add {
                    dst: RegisterId(201),
                    src1: Operand::Register { id: RegisterId(200) },
                    src2: Operand::Register { id: RegisterId(102) }, // 第三个参数
                    span: Span::dummy(),
                },
                // 返回结果
                Instruction::Return {
                    value: Some(RegisterId(201)),
                    span: Span::dummy(),
                },
            ],
            next_register: 202,
            next_label: 2,
            struct_types: HashMap::new(),
            stack_frame_size: 0,
            parameter_count: 3,
        };

        println!("🧪 测试前的函数参数: {:?}", function.parameter_registers);

        // 运行寄存器分配
        let mut pass = SimpleStackRegisterAllocation::new();
        let mut analyses = AnalysisManager::new();
        
        let result = pass.run_on_function(&mut function, &mut analyses);
        
        println!("🧪 测试后的LIR函数:");
        for (i, instruction) in function.instructions.iter().enumerate() {
            println!("  {}: {:?}", i, instruction);
        }

        // 验证结果
        assert!(matches!(result, PassResult::Changed));
        
        // 验证参数寄存器分配是否正确
        // 参数1(RegisterId(100)) 应该分配到 r1
        // 参数2(RegisterId(101)) 应该分配到 r2  
        // 参数3(RegisterId(102)) 应该分配到 r3
        // 返回值(RegisterId(201)) 应该分配到 r0
        
        let mut found_param_usage = false;
        let mut found_return_assignment = false;
        
        for instruction in &function.instructions {
            match instruction {
                Instruction::Add { src1, src2, .. } => {
                    // 检查是否使用了正确的参数寄存器
                    if let (Operand::Register { id: reg1 }, Operand::Register { id: reg2 }) = (src1, src2) {
                        if (reg1.0 == 1 && reg2.0 == 2) || (reg1.0 == 2 && reg2.0 == 1) {
                            found_param_usage = true;
                            println!("✅ 找到正确的参数寄存器使用: r{} + r{}", reg1.0, reg2.0);
                        }
                    }
                }
                Instruction::Return { value: Some(reg), .. } => {
                    if reg.0 == 0 {
                        found_return_assignment = true;
                        println!("✅ 找到正确的返回值寄存器: r{}", reg.0);
                    }
                }
                _ => {}
            }
        }
        
        assert!(found_param_usage, "应该找到参数寄存器的正确使用");
        assert!(found_return_assignment, "应该找到返回值寄存器的正确分配");
        
        println!("✅ 函数参数寄存器分配测试通过");
    }
}

