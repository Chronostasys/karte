use super::{AnalysisManager, FunctionPass, PassResult};
use crate::{Instruction, LirFunction, Operand, Register};
use log::{debug, info};
use std::collections::HashMap;

#[derive(Debug, Clone)]
struct StackSlotInfo {
    alloc_index: usize,
    addr_reg: Register,
    size: usize,
    alignment: usize,
    // 使用区间闭区间 [start, end]
    start: Option<usize>,
    end: Option<usize>,
}

#[derive(Debug, Default)]
struct LinearScanAllocator {
    // 已分配但仍活跃: (addr_reg, end, base_offset, size, alignment)
    active: Vec<(Register, usize, i64, usize, usize)>,
    // 可复用的空闲区间: (base_offset, size, alignment)
    free_list: Vec<(i64, usize, usize)>,
    // 当前最负的偏移（负数，向下增长）
    current_neg_offset: i64,
}

impl LinearScanAllocator {
    fn expire_old_intervals(&mut self, position: usize) {
        self.active.sort_by_key(|(_, end, ..)| *end);
        let mut i = 0;
        while i < self.active.len() {
            if self.active[i].1 < position {
                let (_, _, base, size, align) = self.active.remove(i);
                // 释放到空闲列表，按 exact-size 复用保障对齐
                self.free_list.push((base, size, align));
            } else {
                i += 1;
            }
        }
    }

    fn allocate(&mut self, size: usize, alignment: usize) -> i64 {
        // 简化：统一按 8 字节对齐
        let align = alignment.max(8);
        let aligned_size = size.div_ceil(align) * align;

        // 先尝试从 free_list 复用相同大小与对齐的块
        if let Some(index) = self
            .free_list
            .iter()
            .position(|&(_, sz, al)| sz == aligned_size && al == align)
        {
            let (base, _, _) = self.free_list.remove(index);
            return base;
        }

        // 负向增长新分配
        self.current_neg_offset -= aligned_size as i64;
        self.current_neg_offset
    }

    fn add_active(&mut self, reg: Register, end: usize, base: i64, size: usize, alignment: usize) {
        // 记录规范化的对齐尺寸
        let align = alignment.max(8);
        let aligned_size = size.div_ceil(align) * align;
        self.active.push((reg, end, base, aligned_size, align));
    }
}

pub struct StackFrameLayoutPass;

impl Default for StackFrameLayoutPass {
    fn default() -> Self {
        Self::new()
    }
}

impl StackFrameLayoutPass {
    pub fn new() -> Self {
        Self
    }

    fn collect_stack_slots(&self, function: &LirFunction) -> Vec<StackSlotInfo> {
        let mut slots = vec![];
        for (i, instr) in function.instructions.iter().enumerate() {
            if let Instruction::Alloc {
                dst,
                size,
                alignment,
                allocation_type,
                ..
            } = instr
            {
                if allocation_type.is_stack() {
                    info!(
                        "🔍 StackFrameLayout发现栈槽: 寄存器 {:?}, 大小 {}",
                        dst, size
                    );
                    slots.push(StackSlotInfo {
                        alloc_index: i,
                        addr_reg: *dst,
                        size: *size,
                        alignment: *alignment,
                        start: None,
                        end: None,
                    });
                }
            }
        }
        info!("🔍 StackFrameLayout收集到 {} 个栈槽", slots.len());
        for slot in &slots {
            debug!("  - Slot: {:?}", slot);
        }
        slots
    }

    fn compute_live_ranges(&self, function: &LirFunction, slots: &mut [StackSlotInfo]) {
        for (i, instr) in function.instructions.iter().enumerate() {
            match instr {
                Instruction::Load64 { addr, .. } => {
                    for slot in slots.iter_mut() {
                        if *addr == slot.addr_reg {
                            slot.start = Some(slot.start.map_or(i, |s| s.min(i)));
                            slot.end = Some(slot.end.map_or(i, |e| e.max(i)));
                        }
                    }
                }
                Instruction::Store64 { addr, src, .. } => {
                    for slot in slots.iter_mut() {
                        // 直接对该槽地址进行访问
                        if *addr == slot.addr_reg {
                            slot.start = Some(slot.start.map_or(i, |s| s.min(i)));
                            slot.end = Some(slot.end.map_or(i, |e| e.max(i)));
                        }

                        // 地址逃逸：将该槽的地址值写入了其它内存位置
                        if let Operand::Register { id } = src {
                            if *id == slot.addr_reg {
                                slot.start = Some(slot.start.map_or(i, |s| s.min(i)));
                                // 保守处理：延长到函数末尾，避免与别名间接使用冲突
                                slot.end = Some(function.instructions.len().saturating_sub(1));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn lower_to_fp_offsets(
        &self,
        function: &mut LirFunction,
        slot_offset_map: &HashMap<Register, i64>,
        alloc_offset_map: &HashMap<usize, i64>,
    ) {
        use crate::pass::instruction_transformer::IndexInstructionTransformer;
        use karte_diagnostics::Span;

        // 先分析每个地址寄存器是否仅用于内存地址（Load/Store 的 addr 字段）
        let mut address_only_use: HashMap<Register, bool> = HashMap::new();
        for (addr_reg, _) in slot_offset_map.iter() {
            address_only_use.insert(*addr_reg, true);
        }

        for instr in &function.instructions {
            match instr {
                Instruction::Load64 { addr, .. } => {
                    // 仅当作为地址字段使用时视为纯寻址用途，不修改标记
                    let _ = addr;
                }
                Instruction::Store64 { addr, src, .. } => {
                    // addr 字段为寻址用途，不修改标记；
                    // 但若 src 恰为某个地址寄存器，表示地址值被当作数据使用（地址逃逸），应标记为非纯寻址
                    if let Operand::Register { id: src_reg } = src {
                        if let Some(flag) = address_only_use.get_mut(src_reg) {
                            *flag = false;
                        }
                    }
                    let _ = addr;
                }
                Instruction::Alloc { .. } => {
                    // 注意：Alloc 定义了地址寄存器，但这不是“非地址用途”，不要误判
                }
                _ => {
                    // 其它指令若读/写了地址寄存器，视为非地址用途
                    for (addr_reg, flag) in address_only_use.iter_mut() {
                        let used = instr.get_used_registers().contains(addr_reg);
                        let defined = instr.get_def_register() == Some(*addr_reg);
                        if used || defined {
                            *flag = false;
                        }
                    }
                }
            }
        }

        let mut transformer = IndexInstructionTransformer::new();

        // 两类处理：
        // 1) 地址仅用于内存寻址：直接下沉到 [FP+base_off+offset] 并移除 Alloc
        // 2) 地址逃逸或被当作值：将 Alloc 替换为 add addr_reg, FP, base_off，保留地址寄存器语义

        // 1) 改写所有 load/store 的 addr 字段（仅针对 address_only_use=true 的寄存器）
        for (i, instr) in function.instructions.iter().enumerate() {
            match instr {
                Instruction::Load64 { addr, .. } => {
                    if let Some(base_off) = slot_offset_map.get(addr) {
                        if *address_only_use.get(addr).unwrap_or(&false) {
                            let mut new = instr.clone();
                            if let Instruction::Load64 {
                                addr, offset: off, ..
                            } = &mut new
                            {
                                *addr = Register::Physical(7); // FP
                                *off += *base_off;
                            }
                            transformer.replace(i, new);
                        }
                    }
                }
                Instruction::Store64 { addr, .. } => {
                    if let Some(base_off) = slot_offset_map.get(addr) {
                        if *address_only_use.get(addr).unwrap_or(&false) {
                            let mut new = instr.clone();
                            if let Instruction::Store64 {
                                addr, offset: off, ..
                            } = &mut new
                            {
                                *addr = Register::Physical(7); // FP
                                *off += *base_off;
                            }
                            transformer.replace(i, new);
                        }
                    }
                }
                _ => {}
            }
        }

        // 2) 将需要保留地址值的 Alloc 替换为 add
        for (i, instr) in function.instructions.iter().enumerate() {
            if let Instruction::Alloc {
                dst,
                allocation_type,
                ..
            } = instr
            {
                if allocation_type.is_stack() {
                    // 🔧 关键修复：使用 alloc_index 查找偏移，避免因寄存器重用导致的冲突
                    if let Some(base_off) = alloc_offset_map.get(&i) {
                        if !address_only_use.get(dst).copied().unwrap_or(false) {
                            transformer.replace(
                                i,
                                Instruction::Add {
                                    dst: *dst,
                                    src1: Operand::Register {
                                        id: Register::Physical(7),
                                    }, // FP
                                    src2: Operand::Immediate { value: *base_off },
                                    span: Span::dummy(),
                                },
                            );
                        }
                    }
                }
            }
        }

        // 移除所有已完全下沉（address_only_use=true）的 Alloc 指令
        let mut alloc_indices: Vec<usize> = vec![];
        for (i, instr) in function.instructions.iter().enumerate() {
            if let Instruction::Alloc {
                dst,
                allocation_type,
                ..
            } = instr
            {
                if allocation_type.is_stack()
                    && alloc_offset_map.contains_key(&i)
                    && *address_only_use.get(dst).unwrap_or(&false)
                {
                    alloc_indices.push(i);
                }
            }
        }
        alloc_indices.sort_unstable_by(|a, b| b.cmp(a));
        for idx in alloc_indices {
            transformer.remove(idx);
        }

        transformer.apply_to_function(function);
    }
}

impl FunctionPass for StackFrameLayoutPass {
    fn name(&self) -> &str {
        "stack-frame-layout"
    }

    fn run_on_function(
        &mut self,
        function: &mut LirFunction,
        _analyses: &mut AnalysisManager,
    ) -> PassResult {
        info!("🎯 StackFrameLayout: 函数 {}", function.name);

        // 1) 收集所有栈槽
        let mut slots = self.collect_stack_slots(function);
        if slots.is_empty() {
            return PassResult::Unchanged;
        }

        // 2) 生存期
        self.compute_live_ranges(function, &mut slots);

        // 3) 过滤无使用的槽（仅删除 Alloc）
        // 先移除无用 Alloc
        {
            use crate::pass::instruction_transformer::IndexInstructionTransformer;
            let mut transformer = IndexInstructionTransformer::new();
            let mut dead_allocs: Vec<usize> = vec![];
            for s in &slots {
                if s.start.is_none() || s.end.is_none() {
                    dead_allocs.push(s.alloc_index);
                }
            }
            dead_allocs.sort_unstable_by(|a, b| b.cmp(a));
            for idx in dead_allocs {
                transformer.remove(idx);
            }
            transformer.apply_to_function(function);
        }

        let mut used_slots: Vec<_> = slots
            .iter()
            .filter(|s| s.start.is_some() && s.end.is_some())
            .cloned()
            .collect();

        if used_slots.is_empty() {
            function.stack_frame_size = 0;
            return PassResult::Changed;
        }

        // 4) 线性扫描分配负偏移（相对FP）
        used_slots.sort_by_key(|s| s.start.unwrap());
        let mut allocator = LinearScanAllocator::default();
        let mut offset_map: HashMap<Register, i64> = HashMap::new();
        let mut alloc_offset_map: HashMap<usize, i64> = HashMap::new();

        for slot in &used_slots {
            let start = slot.start.unwrap();
            let end = slot.end.unwrap();
            
            allocator.expire_old_intervals(start);
            let offset = allocator.allocate(slot.size, slot.alignment);
            allocator.add_active(slot.addr_reg, end, offset, slot.size, slot.alignment);
            
            offset_map.insert(slot.addr_reg, offset);
            alloc_offset_map.insert(slot.alloc_index, offset);
            debug!("  - 分配槽 {:?} (idx {}): offset {}, range [{}-{}]", 
                slot.addr_reg, slot.alloc_index, offset, start, end);
        }
        
        function.stack_frame_size = (-allocator.current_neg_offset) as usize;
        // 保持16字节对齐
        function.stack_frame_size = (function.stack_frame_size + 15) & !15;
        
        info!("  - 栈帧大小: {}", function.stack_frame_size);

        self.lower_to_fp_offsets(function, &offset_map, &alloc_offset_map);

        // 6) 设置 stack_frame_size（正数）
        function.stack_frame_size = (-allocator.current_neg_offset) as usize;
        info!(
            "✅ StackFrameLayout 完成，frame_size={}，槽数量={}",
            function.stack_frame_size,
            alloc_offset_map.len()
        );
        PassResult::Changed
    }

    fn required_analyses(&self) -> Vec<&'static str> {
        vec![]
    }

    fn invalidated_analyses(&self) -> Vec<&'static str> {
        vec!["cfg", "def-use"]
    }
}

// 小工具：给 AllocationType 增加 is_stack()
trait AllocationTypeExt {
    fn is_stack(&self) -> bool;
}

impl AllocationTypeExt for crate::AllocationType {
    fn is_stack(&self) -> bool {
        matches!(self, crate::AllocationType::Stack)
    }
}
