//! 代码缓冲区工具
//!
//! 提供高级的代码生成辅助功能，简化机器码的生成

use crate::vm::{VariableInfo, VariableLocation};

use super::compiler_trait::MachineCodeBuffer;

/// 目标架构（用于交叉编译时选择正确的指令编码）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TargetArch {
    #[default]
    Host,
    Riscv64,
}

/// 高级代码缓冲区
///
/// 在基础MachineCodeBuffer之上提供更多便利功能
#[derive(Debug, Clone)]
pub struct CodeBuilder {
    /// 底层代码缓冲区
    buffer: MachineCodeBuffer,

    /// 目标架构（交叉编译用）
    target_arch: TargetArch,

    /// 标签表 (标签名 -> 代码位置)
    labels: std::collections::HashMap<String, usize>,

    /// 全局标签表引用 (可选，用于跨函数标签解析)
    global_labels: Option<std::collections::HashMap<String, usize>>,

    /// 待修补的跳转 (代码位置, 目标标签名, 跳转类型)
    pending_jumps: Vec<PendingJump>,

    /// 待修补的标签地址 (代码位置, 目标标签名)
    pending_label_addresses: Vec<PendingLabelAddress>,

    /// 待修补的ADR指令 (代码位置, 目标标签名, 目标寄存器)
    pending_adrs: Vec<PendingAdr>,

    /// 调试信息
    debug_info: Option<DebugInfoBuilder>,
}

impl CodeBuilder {
    /// 创建新的代码构建器
    pub fn new() -> Self {
        Self {
            buffer: MachineCodeBuffer::new(),
            target_arch: TargetArch::Host,
            labels: std::collections::HashMap::new(),
            global_labels: None,
            pending_jumps: Vec::new(),
            pending_label_addresses: Vec::new(),
            pending_adrs: Vec::new(),
            debug_info: None,
        }
    }

    /// 创建指定目标架构的代码构建器
    pub fn with_target_arch(arch: TargetArch) -> Self {
        Self {
            buffer: MachineCodeBuffer::new(),
            target_arch: arch,
            labels: std::collections::HashMap::new(),
            global_labels: None,
            pending_jumps: Vec::new(),
            pending_label_addresses: Vec::new(),
            pending_adrs: Vec::new(),
            debug_info: None,
        }
    }

    /// 创建带调试信息的代码构建器
    pub fn with_debug_info() -> Self {
        Self {
            buffer: MachineCodeBuffer::new(),
            target_arch: TargetArch::Host,
            labels: std::collections::HashMap::new(),
            global_labels: None,
            pending_jumps: Vec::new(),
            pending_label_addresses: Vec::new(),
            pending_adrs: Vec::new(),
            debug_info: Some(DebugInfoBuilder::new()),
        }
    }

    /// 设置全局标签表
    pub fn set_global_labels(&mut self, global_labels: std::collections::HashMap<String, usize>) {
        self.global_labels = Some(global_labels);
    }

    /// 获取当前代码位置
    pub fn position(&self) -> usize {
        self.buffer.position()
    }

    /// 发射单个字节
    pub fn emit_byte(&mut self, byte: u8) {
        self.buffer.push_byte(byte);
    }

    /// 发射多个字节
    pub fn emit_bytes(&mut self, bytes: &[u8]) {
        self.buffer.push_bytes(bytes);
    }

    /// 发射16位值（小端序）
    pub fn emit_u16(&mut self, value: u16) {
        self.buffer.push_bytes(&value.to_le_bytes());
    }

    /// 发射32位值（小端序）
    pub fn emit_u32(&mut self, value: u32) {
        self.buffer.push_bytes(&value.to_le_bytes());
    }

    /// 发射64位值（小端序）
    pub fn emit_u64(&mut self, value: u64) {
        self.buffer.push_bytes(&value.to_le_bytes());
    }

    /// 发射32位有符号值（小端序）
    pub fn emit_i32(&mut self, value: i32) {
        self.buffer.push_bytes(&value.to_le_bytes());
    }

    /// 发射64位有符号值（小端序）
    pub fn emit_i64(&mut self, value: i64) {
        self.buffer.push_bytes(&value.to_le_bytes());
    }

    /// 定义标签
    pub fn define_label(&mut self, label: &str) -> crate::Result<()> {
        if self.labels.contains_key(label) {
            return Err(format!("标签 '{}' 已经定义", label).into());
        }

        let position = self.buffer.position();
        self.labels.insert(label.to_string(), position);

        if let Some(ref mut debug) = self.debug_info {
            debug.add_label(label, position);
        }

        Ok(())
    }

    /// 发射跳转指令（稍后修补地址）
    pub fn emit_jump(&mut self, jump_type: JumpType, target_label: &str) {
        let patch_position = self.buffer.position();

        if self.target_arch == TargetArch::Riscv64 {
            // RISC-V 跳转指令（交叉编译用）
            match jump_type {
                JumpType::Unconditional => {
                    // JAL x0, offset — 无条件跳转 (21-bit signed)
                    self.emit_u32(0x0000006F); // JAL x0, 0
                }
                JumpType::ConditionalEqual => {
                    // BEQ rs1, rs2, offset — 需要知道寄存器...
                    // 问题：我们不知道要比较哪些寄存器！
                    // RISC-V 条件分支需要两个寄存器操作数
                    // 方案：用 "比较并分支" 模式，在 compile_conditional_jump 中处理
                    // 这里生成占位 BEQ x0, x0, 0（总是跳转）
                    self.emit_u32(0x00000063); // BEQ x0, x0, 0
                }
                JumpType::ConditionalNotEqual => {
                    self.emit_u32(0x00001063); // BNE x0, x0, 0
                }
                JumpType::ConditionalLess => {
                    self.emit_u32(0x00004063); // BLT x0, x0, 0
                }
                JumpType::ConditionalGreater => {
                    self.emit_u32(0x00005063); // BGE x0, x0, 0
                }
                JumpType::ConditionalLessEqual => {
                    self.emit_u32(0x00006063); // BLTU x0, x0, 0
                }
                JumpType::ConditionalGreaterEqual => {
                    self.emit_u32(0x00007063); // BGEU x0, x0, 0
                }
                JumpType::Call => {
                    // AUIPC ra, %hi(offset); JALR ra, ra, %lo(offset)
                    // 简化：用 AUIPC+LD+JALR 模式（通过 pending_label_address 加载目标地址）
                    // 发射 AUIPC ra, 0 (占位)
                    self.emit_u32(0x00000097); // AUIPC ra, 0
                    // LD ra, 0(ra) (占位) — 从代码段加载 64 位地址
                    self.emit_u32(0x0000B083); // LD ra, 0(ra)
                    // JALR x0, ra, 0 — 跳转
                    self.emit_u32(0x00008067); // JALR x0, ra, 0
                    // 占位 64 位地址数据
                    self.emit_u64(0);
                    // 标记需要通过 pending_label_address 修补
                    let addr_patch_pos = patch_position + 12; // 3 条指令后
                    self.pending_label_addresses.push(PendingLabelAddress {
                        patch_position: addr_patch_pos,
                        target_label: target_label.to_string(),
                    });
                    // 不需要 pending_jump，因为地址通过 pending_label_address 修补
                    return;
                }
            }
        } else {
            // 原有实现：x86 或 AArch64（主机架构）
            #[cfg(target_arch = "aarch64")]
            {
                match jump_type {
                    JumpType::Unconditional => { self.emit_u32(0x14000000); }
                    JumpType::ConditionalEqual => { self.emit_u32(0x54000000); }
                    JumpType::ConditionalNotEqual => { self.emit_u32(0x54000001); }
                    JumpType::ConditionalLess => { self.emit_u32(0x5400000B); }
                    JumpType::ConditionalGreater => { self.emit_u32(0x5400000C); }
                    JumpType::ConditionalLessEqual => { self.emit_u32(0x5400000D); }
                    JumpType::ConditionalGreaterEqual => { self.emit_u32(0x5400000A); }
                    JumpType::Call => { self.emit_u32(0x94000000); }
                }
            }

            #[cfg(not(target_arch = "aarch64"))]
            {
                match jump_type {
                    JumpType::Unconditional => {
                        self.emit_byte(0xE9);
                        self.emit_i32(0);
                    }
                    JumpType::ConditionalEqual => {
                        self.emit_bytes(&[0x0F, 0x84]);
                        self.emit_i32(0);
                    }
                    JumpType::ConditionalNotEqual => {
                        self.emit_bytes(&[0x0F, 0x85]);
                        self.emit_i32(0);
                    }
                    JumpType::ConditionalLess => {
                        self.emit_bytes(&[0x0F, 0x8C]);
                        self.emit_i32(0);
                    }
                    JumpType::ConditionalGreater => {
                        self.emit_bytes(&[0x0F, 0x8F]);
                        self.emit_i32(0);
                    }
                    JumpType::ConditionalLessEqual => {
                        self.emit_bytes(&[0x0F, 0x8E]);
                        self.emit_i32(0);
                    }
                    JumpType::ConditionalGreaterEqual => {
                        self.emit_bytes(&[0x0F, 0x8D]);
                        self.emit_i32(0);
                    }
                    JumpType::Call => {
                        self.emit_byte(0xE9);
                        self.emit_i32(0);
                    }
                }
            }
        }

        // 记录待修补的跳转
        self.pending_jumps.push(PendingJump {
            patch_position,
            target_label: target_label.to_string(),
            jump_type,
        });
    }

    /// 发射调用指令
    pub fn emit_call(&mut self, target_label: &str) {
        let patch_position = self.buffer.position();

        // call rel32 - E8 <rel32>
        self.emit_byte(0xE8);
        self.emit_i32(0); // 占位符，稍后修补

        // 记录待修补的调用
        self.pending_jumps.push(PendingJump {
            patch_position: patch_position + 1, // 地址字段的位置
            target_label: target_label.to_string(),
            jump_type: JumpType::Call,
        });
    }

    /// 发射标签地址（稍后修补）
    pub fn emit_label_address(&mut self, target_label: &str) {
        let patch_position = self.buffer.position();

        // 发射8字节占位符（64位地址）
        self.emit_u64(0); // 占位符，稍后修补

        // 记录待修补的标签地址
        self.pending_label_addresses.push(PendingLabelAddress {
            patch_position,
            target_label: target_label.to_string(),
        });
    }

    /// 发射ADR指令（稍后修补）
    pub fn emit_adr(&mut self, dst_register: u8, target_label: &str) {
        let patch_position = self.buffer.position();

        // 发射ADR指令占位符（偏移为0）
        // ADR dst, #0
        let instruction = 0x10000000u32 | (dst_register as u32);
        self.emit_u32(instruction);

        // 记录待修补的ADR指令
        self.pending_adrs.push(PendingAdr {
            patch_position,
            target_label: target_label.to_string(),
            patch_type: AdrPatchType::Adr { dst_register },
        });
    }

    /// 发射ADRP指令（稍后修补）
    pub fn emit_adrp(&mut self, dst_register: u8, target_label: &str) {
        let patch_position = self.buffer.position();

        // ADRP dst, #0
        // 31|30|29|28 27|26 25 24|23 5|4 0
        // 1 |0 |0 |0  0 |1  0  0 |imm19|Rd
        let instruction = 0x90000000u32 | (dst_register as u32);
        self.emit_u32(instruction);

        // 记录待修补的ADRP指令
        self.pending_adrs.push(PendingAdr {
            patch_position,
            target_label: target_label.to_string(),
            patch_type: AdrPatchType::Adrp { dst_register },
        });
    }

    /// 发射ADD指令（用于加载标签地址的低12位）
    pub fn emit_add_reg_label(&mut self, dst_register: u8, target_label: &str) {
        let patch_position = self.buffer.position();

        // ADD dst, dst, #0
        // 31|30|29|28 27 26 25 24 23 22|21 10|9 5|4 0
        // 1 |0 |0 |0  1  0  0  0  1  0 |imm12|Rn |Rd
        let instruction = 0x91000000u32 | ((dst_register as u32) << 5) | (dst_register as u32);
        self.emit_u32(instruction);

        // 记录待修补的ADD指令
        self.pending_adrs.push(PendingAdr {
            patch_position,
            target_label: target_label.to_string(),
            patch_type: AdrPatchType::AddLabel { dst_register },
        });
    }

    /// 发射Store指令的标签地址（稍后修补）
    pub fn emit_store_label_address(&mut self, base_register: u8, offset: i64, target_label: &str) {
        #[cfg(not(target_arch = "aarch64"))]
        {
            let patch_position = self.buffer.position();
            self.emit_u64(0); // 占位符，稍后修补
            self.pending_label_addresses.push(PendingLabelAddress {
                patch_position,
                target_label: target_label.to_string(),
            });
        }
        // AArch64下不做任何事，具体展开在aarch64_compiler.rs
    }

    /// 生成 movabs rax, <label_address> 指令，用于将标签地址加载到 RAX
    /// 占位符在 finalize 时被修补为实际的标签地址
    pub fn emit_movabs_to_rax_with_label(&mut self, target_label: &str) {
        // movabs rax, imm64 = 48 B8 <8 bytes>
        self.emit_byte(0x48);
        self.emit_byte(0xB8);
        let patch_position = self.buffer.position();
        self.emit_u64(0); // 占位符，稍后修补为标签地址
        self.pending_label_addresses.push(PendingLabelAddress {
            patch_position,
            target_label: target_label.to_string(),
        });
    }

    /// 对齐代码到指定边界
    pub fn align(&mut self, alignment: usize) {
        let current_pos = self.buffer.position();
        let aligned_pos = (current_pos + alignment - 1) & !(alignment - 1);
        let padding = aligned_pos - current_pos;

        // 使用NOP指令填充
        for _ in 0..padding {
            self.emit_byte(0x90); // NOP
        }
    }

    /// 完成代码生成（不修补标签，仅返回机器码）
    pub fn finalize(self) -> crate::Result<MachineCodeBuffer> {
        // 直接返回机器码，不修补任何标签引用
        Ok(self.buffer)
    }

    /// 完成代码生成（修补所有跳转，支持可执行内存基址）
    pub fn finalize_with_global_addresses_and_exec_base(
        mut self,
        global_addresses: Option<&std::collections::HashMap<String, *const u8>>,
        exec_base: usize,
    ) -> crate::Result<MachineCodeBuffer> {
        log::debug!("🔧 开始修补跳转，exec_base: 0x{:016X}", exec_base);

        // 修补所有待处理的跳转
        for pending in &self.pending_jumps {
            let target_addr = self.find_label_address(&pending.target_label, global_addresses)?;

            log::debug!(
                "🔧 修补跳转 '{}': 目标地址=0x{:016X}, patch_position={}, 跳转类型={:?}",
                pending.target_label,
                target_addr,
                pending.patch_position,
                pending.jump_type
            );

            #[cfg(target_arch = "aarch64")]
            {
                // AArch64跳转修补
                let current_pos = exec_base + pending.patch_position; // 转换为可执行内存中的真实地址

                // 🔧 修复：如果target_addr是相对偏移（本地标签），需要转换为绝对地址
                let target_addr_absolute = if target_addr < 0x10000 {
                    // 相对偏移（本地标签），转换为绝对地址
                    exec_base + target_addr
                } else {
                    // 已经是绝对地址（全局标签）
                    target_addr
                };

                // 计算相对偏移（以字为单位，4字节对齐）
                let relative_offset = ((target_addr_absolute as i64) - (current_pos as i64)) >> 2;

                log::debug!("🔧 修补跳转详情: {} -> 目标: 0x{:016X}, 当前位置: 0x{:016X}, 字节偏移: {}, 相对偏移(字): {}", 
                         pending.target_label, target_addr, current_pos,
                         (target_addr as i64) - (current_pos as i64), relative_offset);

                // 🔧 关键调试：检查原始指令内容
                let bytes = self.buffer.as_bytes();
                let instruction_bytes = &bytes[pending.patch_position..pending.patch_position + 4];
                let original_instruction = u32::from_le_bytes([
                    instruction_bytes[0],
                    instruction_bytes[1],
                    instruction_bytes[2],
                    instruction_bytes[3],
                ]);
                log::debug!(
                    "🔧 原始指令: 0x{:08X} (位置: {})",
                    original_instruction,
                    pending.patch_position
                );

                match pending.jump_type {
                    JumpType::Unconditional | JumpType::Call => {
                        // B/BL指令：26位偏移（指令为4字节对齐）
                        if !(-(1 << 25)..(1 << 25)).contains(&relative_offset) {
                            return Err(format!("跳转距离太远: {} 字节", relative_offset << 2).into());
                        }

                        // 获取原始指令并修补偏移
                        let mut instruction = original_instruction;

                        // 清除旧偏移并设置新偏移（低26位）
                        instruction &= 0xFC000000; // 清除低26位
                        instruction |= (relative_offset as u32) & 0x03FFFFFF; // 设置新偏移

                        log::debug!(
                            "🔧 修补无条件跳转: 原始指令: 0x{:08X}, 修补后: 0x{:08X}, 偏移值: {}",
                            original_instruction,
                            instruction,
                            relative_offset
                        );

                        // 写回修改后的指令
                        self.buffer
                            .write_bytes_at(pending.patch_position, &instruction.to_le_bytes())?;
                    }
                    _ => {
                        // 条件跳转指令：19位偏移
                        if !(-(1 << 18)..(1 << 18)).contains(&relative_offset) {
                            return Err(format!("条件跳转距离太远: {} 字节", relative_offset << 2).into());
                        }

                        let mut instruction = original_instruction;

                        // 清除旧偏移并设置新偏移（5-23位）
                        instruction &= 0xFF00001F; // 保留条件码和指令格式
                        instruction |= ((relative_offset as u32) & 0x7FFFF) << 5; // 设置新偏移

                        log::debug!(
                            "🔧 修补条件跳转: 原始指令: 0x{:08X}, 修补后: 0x{:08X}, 偏移值: {}",
                            original_instruction,
                            instruction,
                            relative_offset
                        );

                        // 写回修改后的指令
                        self.buffer
                            .write_bytes_at(pending.patch_position, &instruction.to_le_bytes())?;
                    }
                }

                // 🔧 验证修补结果
                let verification_bytes = self.buffer.as_bytes();
                let verification_bytes =
                    &verification_bytes[pending.patch_position..pending.patch_position + 4];
                let verification_instruction = u32::from_le_bytes([
                    verification_bytes[0],
                    verification_bytes[1],
                    verification_bytes[2],
                    verification_bytes[3],
                ]);
                log::debug!(
                    "🔧 修补验证: 写入后指令: 0x{:08X}",
                    verification_instruction
                );
            }

            #[cfg(not(target_arch = "aarch64"))]
            {
                // x86/x64跳转修补（原有逻辑）
                let patch_pos = pending.patch_position + pending.jump_type.instruction_size() - 4;
                let current_pos = exec_base + patch_pos + 4; // 相对于偏移字段结束位置
                let relative_offset = (target_addr as i64) - (current_pos as i64);

                // 检查偏移是否在32位范围内
                if relative_offset < i32::MIN as i64 || relative_offset > i32::MAX as i64 {
                    return Err(format!("跳转距离太远: {}", relative_offset).into());
                }

                // 将偏移写入代码
                let offset_bytes = (relative_offset as i32).to_le_bytes();
                self.buffer.write_bytes_at(patch_pos, &offset_bytes)?;
            }
        }

        log::debug!("🔧 跳转修补完成，继续修补标签地址");

        // 修补所有待处理的标签地址
        for pending in &self.pending_label_addresses {
            let target_addr = self.find_label_address(&pending.target_label, global_addresses)?;

            // 将标签地址写入代码
            let address_bytes = (target_addr as u64).to_le_bytes();
            log::debug!(
                "🔧 修补标签地址: {} -> 位置{} (0x{:016X})",
                pending.target_label,
                target_addr,
                target_addr as u64
            );
            self.buffer
                .write_bytes_at(pending.patch_position, &address_bytes)?;
        }

        log::debug!("🔧 开始修补ADR指令");

        // 修补所有待处理的ADR指令
        for pending in &self.pending_adrs {
            let target_addr = self.find_label_address(&pending.target_label, global_addresses)?;

            log::debug!(
                "🔧 修补ADR指令: {} -> 目标地址: 0x{:016X}",
                pending.target_label,
                target_addr
            );

            match &pending.patch_type {
                AdrPatchType::Adrp { dst_register } => {
                    // ADRP指令修补：计算页地址偏移
                    let current_pos = exec_base + pending.patch_position;
                    let current_page = current_pos & !0xFFF; // 当前指令所在页
                    let target_page = target_addr & !0xFFF; // 目标地址所在页
                    let page_offset = ((target_page as i64) - (current_page as i64)) >> 12; // 转换为页偏移

                    log::debug!("🔧 ADRP修补: 当前位置: 0x{:016X}, 当前页: 0x{:016X}, 目标页: 0x{:016X}, 页偏移: {}", 
                             current_pos, current_page, target_page, page_offset);

                    // 检查页偏移是否在21位有符号范围内
                    if !(-(1 << 20)..(1 << 20)).contains(&page_offset) {
                        return Err(format!(
                            "ADRP页偏移超出范围: {} (应在 ±1M 范围内)",
                            page_offset
                        ).into());
                    }

                    let bytes = self.buffer.as_bytes();
                    let instruction_bytes =
                        &bytes[pending.patch_position..pending.patch_position + 4];
                    let mut instruction = u32::from_le_bytes([
                        instruction_bytes[0],
                        instruction_bytes[1],
                        instruction_bytes[2],
                        instruction_bytes[3],
                    ]);

                    // 清除旧偏移并设置新偏移
                    instruction &= 0x9F00001F; // 保留指令格式和目标寄存器

                    // 正确处理21位有符号立即数
                    // AArch64 ADRP指令格式：immhi(19位) + immlo(2位)
                    let imm = if page_offset < 0 {
                        // 负数：使用补码表示
                        (0x200000 - (-page_offset as u32)) & 0x1FFFFF
                    } else {
                        // 正数：直接使用
                        page_offset as u32
                    };

                    // 分离immhi和immlo
                    let immhi = (imm >> 2) & 0x7FFFF; // 高19位
                    let immlo = imm & 0x3; // 低2位

                    instruction |= (immhi << 5) | (immlo << 29); // 设置immhi和immlo

                    log::debug!("🔧 ADRP指令修补: 原始: 0x{:08X}, 修补后: 0x{:08X}, 页偏移: {}, imm: 0x{:X}, immhi: 0x{:X}, immlo: 0x{:X}", 
                             u32::from_le_bytes([instruction_bytes[0], instruction_bytes[1], instruction_bytes[2], instruction_bytes[3]]),
                             instruction, page_offset, imm, immhi, immlo);

                    // 写回修改后的指令
                    self.buffer
                        .write_bytes_at(pending.patch_position, &instruction.to_le_bytes())?;
                }
                AdrPatchType::AddLabel { dst_register } => {
                    // ADD指令修补：设置页内偏移（低12位）
                    let page_offset = target_addr & 0xFFF;

                    log::debug!(
                        "🔧 ADD修补: 目标地址: 0x{:016X}, 页内偏移: 0x{:X}",
                        target_addr,
                        page_offset
                    );

                    let bytes = self.buffer.as_bytes();
                    let instruction_bytes =
                        &bytes[pending.patch_position..pending.patch_position + 4];
                    let mut instruction = u32::from_le_bytes([
                        instruction_bytes[0],
                        instruction_bytes[1],
                        instruction_bytes[2],
                        instruction_bytes[3],
                    ]);

                    // 清除旧偏移并设置新偏移
                    instruction &= 0xFFC003FF; // 保留指令格式和寄存器
                    instruction |= ((page_offset as u32) & 0xFFF) << 10; // 设置页内偏移

                    log::debug!(
                        "🔧 ADD指令修补: 原始: 0x{:08X}, 修补后: 0x{:08X}",
                        u32::from_le_bytes([
                            instruction_bytes[0],
                            instruction_bytes[1],
                            instruction_bytes[2],
                            instruction_bytes[3]
                        ]),
                        instruction
                    );

                    // 写回修改后的指令
                    self.buffer
                        .write_bytes_at(pending.patch_position, &instruction.to_le_bytes())?;
                }
                AdrPatchType::Store {
                    base_register,
                    offset,
                } => {
                    // 存储标签地址修补
                    #[cfg(target_arch = "aarch64")]
                    {
                        // 在AArch64上，需要修补ADRP和ADD指令的标签偏移
                        let current_pos = exec_base + pending.patch_position;
                        let target_address = target_addr;

                        // 修补ADRP指令（第一条指令）
                        let page_offset =
                            (target_address & !0xFFF) as i64 - (current_pos & !0xFFF) as i64;
                        let page_offset = page_offset >> 12; // 转换为页偏移

                        // 读取ADRP指令
                        let bytes = self.buffer.as_bytes();
                        let adrp_instruction_bytes = [
                            bytes[pending.patch_position],
                            bytes[pending.patch_position + 1],
                            bytes[pending.patch_position + 2],
                            bytes[pending.patch_position + 3],
                        ];
                        let mut adrp_instruction = u32::from_le_bytes(adrp_instruction_bytes);

                        // 修补ADRP的页偏移（imm21）
                        let imm_lo = (page_offset as u32) & 0x3;
                        let imm_hi = ((page_offset as u32) >> 2) & 0x7FFFF;
                        adrp_instruction &= 0x9F00001F; // 清除旧偏移
                        adrp_instruction |= (imm_lo << 29) | (imm_hi << 5);

                        // 读取ADD指令
                        let add_pos = pending.patch_position + 4;
                        let add_instruction_bytes = [
                            bytes[add_pos],
                            bytes[add_pos + 1],
                            bytes[add_pos + 2],
                            bytes[add_pos + 3],
                        ];
                        let mut add_instruction = u32::from_le_bytes(add_instruction_bytes);

                        // 修补ADD的立即数（imm12）
                        let page_internal_offset = target_address & 0xFFF;
                        add_instruction &= 0xFFC003FF; // 清除旧立即数
                        add_instruction |= ((page_internal_offset as u32) & 0xFFF) << 10;

                        // 写回修补后的指令
                        self.buffer.write_bytes_at(
                            pending.patch_position,
                            &adrp_instruction.to_le_bytes(),
                        )?;
                        self.buffer
                            .write_bytes_at(add_pos, &add_instruction.to_le_bytes())?;
                    }

                    #[cfg(not(target_arch = "aarch64"))]
                    {
                        // x86/x64版本：直接写入地址
                        let address = target_addr as u64;
                        self.buffer
                            .write_bytes_at(pending.patch_position, &address.to_le_bytes())?;
                    }
                }
                _ => {
                    return Err(format!("不支持的地址修补类型: {:?}", pending.patch_type).into());
                }
            }
        }

        Ok(self.buffer)
    }

    /// 完成代码生成（修补所有跳转）
    pub fn finalize_with_global_addresses(
        self,
        global_addresses: Option<&std::collections::HashMap<String, *const u8>>,
    ) -> crate::Result<MachineCodeBuffer> {
        // 调用新方法，使用0作为可执行内存基址（兼容旧代码）
        self.finalize_with_global_addresses_and_exec_base(global_addresses, 0)
    }

    /// 查找标签真实地址（优先查全局地址表）
    fn find_label_address(
        &self,
        label_name: &str,
        global_addresses: Option<&std::collections::HashMap<String, *const u8>>,
    ) -> crate::Result<usize> {
        if let Some(global) = global_addresses {
            if let Some(&addr) = global.get(label_name) {
                return Ok(addr as usize);
            }
        }
        // 回退到本地偏移
        self.find_label_position(label_name)
    }

    /// 查找标签位置（先查找本地标签，再查找全局标签）
    fn find_label_position(&self, label_name: &str) -> crate::Result<usize> {
        // 首先查找本地标签
        if let Some(&position) = self.labels.get(label_name) {
            return Ok(position);
        }

        // 然后查找全局标签
        if let Some(ref global_labels) = self.global_labels {
            if let Some(&position) = global_labels.get(label_name) {
                return Ok(position);
            }
        }

        Err(format!("未定义的标签: {}", label_name).into())
    }

    /// 添加源代码行号信息
    pub fn add_source_line(&mut self, line_number: usize) {
        if let Some(ref mut debug) = self.debug_info {
            debug.add_source_line(self.buffer.position(), line_number);
        }
    }

    /// 添加变量信息
    pub fn add_variable(&mut self, name: &str, location: VariableLocation, scope_start: usize) {
        if let Some(ref mut debug) = self.debug_info {
            debug.add_variable(name, location, scope_start, self.buffer.position());
        }
    }

    /// 获取代码大小
    pub fn size(&self) -> usize {
        self.buffer.len()
    }

    /// 检查标签是否已定义
    pub fn is_label_defined(&self, label: &str) -> bool {
        self.labels.contains_key(label)
    }

    /// 导出所有label及其偏移
    pub fn exported_labels(&self) -> &std::collections::HashMap<String, usize> {
        &self.labels
    }
    /// 导出所有待修补的跳转
    pub fn exported_pending_jumps(&self) -> &Vec<PendingJump> {
        &self.pending_jumps
    }
    /// 手动添加一个 pending jump（用于 RISC-V 等架构直接生成分支指令后记录）
    pub fn add_pending_jump_raw(&mut self, patch_position: usize, target_label: String, jump_type: JumpType) {
        self.pending_jumps.push(PendingJump {
            patch_position,
            target_label,
            jump_type,
        });
    }
    /// 导出所有待修补的标签地址
    pub fn exported_pending_label_addresses(&self) -> &Vec<PendingLabelAddress> {
        &self.pending_label_addresses
    }
    /// 导出所有待修补的ADR指令
    pub fn exported_pending_adrs(&self) -> &Vec<PendingAdr> {
        &self.pending_adrs
    }
    /// 导出调试信息
    pub fn exported_debug_info(&self) -> Option<&DebugInfoBuilder> {
        self.debug_info.as_ref()
    }

    pub fn set_buffer(&mut self, buffer: MachineCodeBuffer) {
        self.buffer = buffer;
    }
    pub fn set_labels(&mut self, labels: std::collections::HashMap<String, usize>) {
        self.labels = labels;
    }
    pub fn set_pending_jumps(&mut self, jumps: Vec<PendingJump>) {
        self.pending_jumps = jumps;
    }
    pub fn set_pending_label_addresses(&mut self, addrs: Vec<PendingLabelAddress>) {
        self.pending_label_addresses = addrs;
    }
    pub fn set_pending_adrs(&mut self, adrs: Vec<PendingAdr>) {
        self.pending_adrs = adrs;
    }
    pub fn set_debug_info(&mut self, debug: Option<DebugInfoBuilder>) {
        self.debug_info = debug;
    }

    pub fn exported_buffer(&self) -> &MachineCodeBuffer {
        &self.buffer
    }
}

impl Default for CodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 待修补的跳转信息
#[derive(Debug, Clone)]
pub struct PendingJump {
    /// 需要修补的代码位置
    pub patch_position: usize,
    /// 目标标签名
    pub target_label: String,
    /// 跳转类型
    pub jump_type: JumpType,
}

/// 待修补的标签地址信息
#[derive(Debug, Clone)]
pub struct PendingLabelAddress {
    /// 需要修补的代码位置
    pub patch_position: usize,
    /// 目标标签名
    pub target_label: String,
}

/// 待修补的ADR指令信息
#[derive(Debug, Clone)]
pub struct PendingAdr {
    /// 需要修补的代码位置
    pub patch_position: usize,
    /// 目标标签名
    pub target_label: String,
    /// 修补类型
    pub patch_type: AdrPatchType,
}

/// ADR指令修补类型
#[derive(Debug, Clone)]
pub enum AdrPatchType {
    /// ADR指令
    Adr { dst_register: u8 },
    /// ADRP指令
    Adrp { dst_register: u8 },
    /// ADD指令（用于标签地址的低12位）
    AddLabel { dst_register: u8 },
    /// Store指令
    Store { base_register: u8, offset: i64 },
}

impl AdrPatchType {
    /// 获取目标寄存器
    pub fn get_dst_register(&self) -> u8 {
        match self {
            AdrPatchType::Adr { dst_register } => *dst_register,
            AdrPatchType::Adrp { dst_register } => *dst_register,
            AdrPatchType::AddLabel { dst_register } => *dst_register,
            AdrPatchType::Store { base_register, .. } => *base_register,
        }
    }
}

/// 跳转类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpType {
    /// 无条件跳转
    Unconditional,
    /// 条件跳转：相等
    ConditionalEqual,
    /// 条件跳转：不相等
    ConditionalNotEqual,
    /// 条件跳转：小于
    ConditionalLess,
    /// 条件跳转：大于
    ConditionalGreater,
    /// 条件跳转：小于等于
    ConditionalLessEqual,
    /// 条件跳转：大于等于
    ConditionalGreaterEqual,
    /// 函数调用
    Call,
}

impl JumpType {
    /// 获取指令大小（字节）
    pub fn instruction_size(&self) -> usize {
        match self {
            JumpType::Unconditional => 5, // E9 + 4字节地址
            JumpType::Call => 5,          // E9 (jmp) + 4字节地址 (使用 jmp 代替 call)
            _ => 6,                       // 0F XX + 4字节地址
        }
    }
}

/// 调试信息构建器
#[derive(Debug, Clone)]
pub struct DebugInfoBuilder {
    /// 源代码行号映射
    pub line_map: std::collections::HashMap<usize, usize>,
    /// 变量信息
    pub variables: Vec<VariableInfo>,
    /// 标签位置
    pub labels: std::collections::HashMap<String, usize>,
}

impl DebugInfoBuilder {
    fn new() -> Self {
        Self {
            line_map: std::collections::HashMap::new(),
            variables: Vec::new(),
            labels: std::collections::HashMap::new(),
        }
    }

    fn add_source_line(&mut self, code_offset: usize, line_number: usize) {
        self.line_map.insert(code_offset, line_number);
    }

    fn add_variable(
        &mut self,
        name: &str,
        location: VariableLocation,
        scope_start: usize,
        scope_end: usize,
    ) {
        self.variables.push(VariableInfo {
            name: name.to_string(),
            location,
            scope: (scope_start, scope_end),
        });
    }

    fn add_label(&mut self, label: &str, position: usize) {
        self.labels.insert(label.to_string(), position);
    }
}

// ============= 独立的修补函数 =============

/// 在可执行内存上直接进行跳转修补（避免二次编译）
///
/// 从 finalize_with_global_addresses_and_exec_base 提取的核心修补逻辑，
/// 可以直接在已分配的可执行内存上进行修补，无需重新编译整个函数。
///
/// # 参数
/// - `memory_ptr`: 可执行内存的起始地址
/// - `exec_base`: 执行时的基地址（用于计算相对偏移）
/// - `labels`: 本函数内的 label 偏移表（相对于函数起始的偏移）
/// - `global_labels`: 全局 label 地址表（绝对地址）
/// - `pending_jumps`: 待修补的跳转列表
/// - `pending_adrs`: 待修补的 ADR 指令列表
/// - `pending_label_addresses`: 待修补的标签地址列表
pub fn patch_executable_memory(
    memory_ptr: *mut u8,
    exec_base: usize,
    labels: &std::collections::HashMap<String, usize>,
    global_labels: &std::collections::HashMap<String, usize>,
    pending_jumps: &[PendingJump],
    pending_adrs: &[PendingAdr],
    pending_label_addresses: &[PendingLabelAddress],
) -> crate::Result<()> {
    log::debug!("🔧 开始原地修补，exec_base: 0x{:016X}", exec_base);

    // 辅助函数：查找 label 地址
    let find_label_address = |label_name: &str| -> crate::Result<usize> {
        // 优先查找全局标签表（绝对地址）
        if let Some(&addr) = global_labels.get(label_name) {
            return Ok(addr);
        }
        // 回退到本地标签表（相对偏移，需要加上 exec_base）
        if let Some(&offset) = labels.get(label_name) {
            return Ok(exec_base + offset);
        }
        Err(format!("未定义的标签: {}", label_name).into())
    };

    // 1. 修补所有待处理的跳转
    for pending in pending_jumps {
        let target_addr = find_label_address(&pending.target_label)?;

        log::debug!(
            "🔧 修补跳转 '{}': 目标地址=0x{:016X}, patch_position={}, 跳转类型={:?}",
            pending.target_label,
            target_addr,
            pending.patch_position,
            pending.jump_type
        );

        #[cfg(target_arch = "aarch64")]
        {
            let current_pos = exec_base + pending.patch_position;

            // 计算相对偏移（以字为单位，4字节对齐）
            let relative_offset = ((target_addr as i64) - (current_pos as i64)) >> 2;

            // 读取原始指令
            let original_instruction = unsafe { read_u32_at(memory_ptr, pending.patch_position) };

            log::debug!(
                "🔧 修补跳转详情: {} -> 目标: 0x{:016X}, 当前位置: 0x{:016X}, 相对偏移(字): {}",
                pending.target_label,
                target_addr,
                current_pos,
                relative_offset
            );

            match pending.jump_type {
                JumpType::Unconditional | JumpType::Call => {
                    // B/BL指令：26位偏移
                    if !(-(1 << 25)..(1 << 25)).contains(&relative_offset) {
                        return Err(format!("跳转距离太远: {} 字节", relative_offset << 2).into());
                    }

                    let mut instruction = original_instruction;
                    instruction &= 0xFC000000; // 清除低26位
                    instruction |= (relative_offset as u32) & 0x03FFFFFF; // 设置新偏移

                    log::debug!(
                        "🔧 修补无条件跳转: 原始: 0x{:08X}, 修补后: 0x{:08X}",
                        original_instruction,
                        instruction
                    );

                    unsafe { write_u32_at(memory_ptr, pending.patch_position, instruction) };
                }
                _ => {
                    // 条件跳转指令：19位偏移
                    if !(-(1 << 18)..(1 << 18)).contains(&relative_offset) {
                        return Err(format!("条件跳转距离太远: {} 字节", relative_offset << 2).into());
                    }

                    let mut instruction = original_instruction;
                    instruction &= 0xFF00001F; // 保留条件码和指令格式
                    instruction |= ((relative_offset as u32) & 0x7FFFF) << 5; // 设置新偏移

                    log::debug!(
                        "🔧 修补条件跳转: 原始: 0x{:08X}, 修补后: 0x{:08X}",
                        original_instruction,
                        instruction
                    );

                    unsafe { write_u32_at(memory_ptr, pending.patch_position, instruction) };
                }
            }
        }

        #[cfg(not(target_arch = "aarch64"))]
        {
            // x86/x64跳转修补
            let patch_pos = pending.patch_position + pending.jump_type.instruction_size() - 4;
            let current_pos = exec_base + patch_pos + 4;
            let relative_offset = (target_addr as i64) - (current_pos as i64);

            if relative_offset < i32::MIN as i64 || relative_offset > i32::MAX as i64 {
                return Err(format!("跳转距离太远: {}", relative_offset).into());
            }

            let offset_bytes = (relative_offset as i32).to_le_bytes();
            unsafe {
                for (i, &byte) in offset_bytes.iter().enumerate() {
                    *memory_ptr.add(patch_pos + i) = byte;
                }
            }
        }
    }

    log::debug!("🔧 跳转修补完成，继续修补标签地址");

    // 2. 修补所有待处理的标签地址
    for pending in pending_label_addresses {
        let target_addr = find_label_address(&pending.target_label)?;

        log::debug!(
            "🔧 修补标签地址: {} -> 位置{} (0x{:016X})",
            pending.target_label,
            target_addr,
            target_addr as u64
        );

        let address_bytes = (target_addr as u64).to_le_bytes();
        unsafe {
            for (i, &byte) in address_bytes.iter().enumerate() {
                *memory_ptr.add(pending.patch_position + i) = byte;
            }
        }
    }

    log::debug!("🔧 开始修补ADR指令");

    // 3. 修补所有待处理的ADR指令
    for pending in pending_adrs {
        let target_addr = find_label_address(&pending.target_label)?;

        log::debug!(
            "🔧 修补ADR指令: {} -> 目标地址: 0x{:016X}",
            pending.target_label,
            target_addr
        );

        match &pending.patch_type {
            AdrPatchType::Adrp { dst_register: _ } => {
                #[cfg(target_arch = "aarch64")]
                {
                    let current_pos = exec_base + pending.patch_position;
                    let current_page = current_pos & !0xFFF;
                    let target_page = target_addr & !0xFFF;
                    let page_offset = ((target_page as i64) - (current_page as i64)) >> 12;



                    if !(-(1 << 20)..(1 << 20)).contains(&page_offset) {
                        return Err(format!(
                            "ADRP页偏移超出范围: {} (应在 ±1M 范围内)",
                            page_offset
                        ).into());
                    }

                    let mut instruction =
                        unsafe { read_u32_at(memory_ptr, pending.patch_position) };
                    instruction &= 0x9F00001F;

                    let imm = if page_offset < 0 {
                        (0x200000 - (-page_offset as u32)) & 0x1FFFFF
                    } else {
                        page_offset as u32
                    };

                    let immhi = (imm >> 2) & 0x7FFFF;
                    let immlo = imm & 0x3;
                    instruction |= (immhi << 5) | (immlo << 29);


                    unsafe { write_u32_at(memory_ptr, pending.patch_position, instruction) };
                }
            }
            AdrPatchType::AddLabel { dst_register: _ } => {
                #[cfg(target_arch = "aarch64")]
                {
                    let page_offset = target_addr & 0xFFF;

                    let mut instruction =
                        unsafe { read_u32_at(memory_ptr, pending.patch_position) };
                    instruction &= 0xFFC003FF;
                    instruction |= ((page_offset as u32) & 0xFFF) << 10;


                    unsafe { write_u32_at(memory_ptr, pending.patch_position, instruction) };
                }
            }
            AdrPatchType::Store {
                base_register: _,
                offset: _,
            } => {
                #[cfg(target_arch = "aarch64")]
                {
                    let current_pos = exec_base + pending.patch_position;

                    // 修补ADRP指令
                    let page_offset = (target_addr & !0xFFF) as i64 - (current_pos & !0xFFF) as i64;
                    let page_offset = page_offset >> 12;

                    let mut adrp_instruction =
                        unsafe { read_u32_at(memory_ptr, pending.patch_position) };
                    let imm_lo = (page_offset as u32) & 0x3;
                    let imm_hi = ((page_offset as u32) >> 2) & 0x7FFFF;
                    adrp_instruction &= 0x9F00001F;
                    adrp_instruction |= (imm_lo << 29) | (imm_hi << 5);

                    // 修补ADD指令
                    let add_pos = pending.patch_position + 4;
                    let mut add_instruction = unsafe { read_u32_at(memory_ptr, add_pos) };
                    let page_internal_offset = target_addr & 0xFFF;
                    add_instruction &= 0xFFC003FF;
                    add_instruction |= ((page_internal_offset as u32) & 0xFFF) << 10;

                    unsafe {
                        write_u32_at(memory_ptr, pending.patch_position, adrp_instruction);
                        write_u32_at(memory_ptr, add_pos, add_instruction);
                    }
                }

                #[cfg(not(target_arch = "aarch64"))]
                {
                    let address_bytes = (target_addr as u64).to_le_bytes();
                    unsafe {
                        for (i, &byte) in address_bytes.iter().enumerate() {
                            *memory_ptr.add(pending.patch_position + i) = byte;
                        }
                    }
                }
            }
            AdrPatchType::Adr { dst_register: _ } => {
                // ADR指令修补（如果需要）
                log::warn!("ADR指令修补暂未实现");
            }
        }
    }

    log::debug!("🔧 原地修补完成");
    Ok(())
}

/// 从内存中读取 u32（小端序）
#[inline]
unsafe fn read_u32_at(ptr: *mut u8, offset: usize) -> u32 {
    (ptr.add(offset) as *const u32).read_unaligned()
}

/// 向内存中写入 u32（小端序）
#[inline]
unsafe fn write_u32_at(ptr: *mut u8, offset: usize, value: u32) {
    (ptr.add(offset) as *mut u32).write_unaligned(value);
}
