//! 代码缓冲区工具
//!
//! 提供高级的代码生成辅助功能，简化机器码的生成

use super::compiler_trait::MachineCodeBuffer;

/// 高级代码缓冲区
/// 
/// 在基础MachineCodeBuffer之上提供更多便利功能
#[derive(Debug, Clone)]
pub struct CodeBuilder {
    /// 底层代码缓冲区
    buffer: MachineCodeBuffer,
    
    /// 标签表 (标签名 -> 代码位置)
    labels: std::collections::HashMap<String, usize>,
    
    /// 待修补的跳转 (代码位置, 目标标签名, 跳转类型)
    pending_jumps: Vec<PendingJump>,
    
    /// 调试信息
    debug_info: Option<DebugInfoBuilder>,
}

impl CodeBuilder {
    /// 创建新的代码构建器
    pub fn new() -> Self {
        Self {
            buffer: MachineCodeBuffer::new(),
            labels: std::collections::HashMap::new(),
            pending_jumps: Vec::new(),
            debug_info: None,
        }
    }

    /// 创建带调试信息的代码构建器
    pub fn with_debug_info() -> Self {
        Self {
            buffer: MachineCodeBuffer::new(),
            labels: std::collections::HashMap::new(),
            pending_jumps: Vec::new(),
            debug_info: Some(DebugInfoBuilder::new()),
        }
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
    pub fn define_label(&mut self, label: &str) -> Result<(), String> {
        if self.labels.contains_key(label) {
            return Err(format!("标签 '{}' 已经定义", label));
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
        
        // 发射跳转指令的操作码和占位符地址
        match jump_type {
            JumpType::Unconditional => {
                // jmp rel32 - E9 <rel32>
                self.emit_byte(0xE9);
                self.emit_i32(0); // 占位符，稍后修补
            }
            JumpType::ConditionalEqual => {
                // je rel32 - 0F 84 <rel32>
                self.emit_bytes(&[0x0F, 0x84]);
                self.emit_i32(0); // 占位符，稍后修补
            }
            JumpType::ConditionalNotEqual => {
                // jne rel32 - 0F 85 <rel32>
                self.emit_bytes(&[0x0F, 0x85]);
                self.emit_i32(0); // 占位符，稍后修补
            }
            JumpType::ConditionalLess => {
                // jl rel32 - 0F 8C <rel32>
                self.emit_bytes(&[0x0F, 0x8C]);
                self.emit_i32(0); // 占位符，稍后修补
            }
            JumpType::ConditionalGreater => {
                // jg rel32 - 0F 8F <rel32>
                self.emit_bytes(&[0x0F, 0x8F]);
                self.emit_i32(0); // 占位符，稍后修补
            }
            JumpType::ConditionalLessEqual => {
                // jle rel32 - 0F 8E <rel32>
                self.emit_bytes(&[0x0F, 0x8E]);
                self.emit_i32(0); // 占位符，稍后修补
            }
            JumpType::ConditionalGreaterEqual => {
                // jge rel32 - 0F 8D <rel32>
                self.emit_bytes(&[0x0F, 0x8D]);
                self.emit_i32(0); // 占位符，稍后修补
            }
            JumpType::Call => {
                // call rel32 - E8 <rel32>
                self.emit_byte(0xE8);
                self.emit_i32(0); // 占位符，稍后修补
            }
        }
        
        // 记录待修补的跳转
        self.pending_jumps.push(PendingJump {
            patch_position: patch_position + jump_type.instruction_size() - 4, // 地址字段的位置
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

    /// 完成代码生成（修补所有跳转）
    pub fn finalize(mut self) -> Result<MachineCodeBuffer, String> {
        // 修补所有待处理的跳转
        for pending in &self.pending_jumps {
            let target_pos = self.labels.get(&pending.target_label)
                .ok_or_else(|| format!("未定义的标签: {}", pending.target_label))?;
            
            // 计算相对偏移
            let current_pos = pending.patch_position + 4; // 相对于指令结束位置
            let relative_offset = (*target_pos as i64) - (current_pos as i64);
            
            // 检查偏移是否在32位范围内
            if relative_offset < i32::MIN as i64 || relative_offset > i32::MAX as i64 {
                return Err(format!(
                    "跳转偏移超出32位范围: {} (从 {} 到 {})",
                    relative_offset, current_pos, target_pos
                ));
            }
            
            // 修补地址
            let offset_bytes = (relative_offset as i32).to_le_bytes();
            self.buffer.write_bytes_at(pending.patch_position, &offset_bytes)?;
        }
        
        Ok(self.buffer)
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
}

impl Default for CodeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 待修补的跳转信息
#[derive(Debug, Clone)]
struct PendingJump {
    /// 需要修补的代码位置
    patch_position: usize,
    /// 目标标签名
    target_label: String,
    /// 跳转类型
    jump_type: JumpType,
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
            JumpType::Call => 5, // E8 + 4字节地址
            _ => 6, // 0F XX + 4字节地址
        }
    }
}

/// 调试信息构建器
#[derive(Debug, Clone)]
struct DebugInfoBuilder {
    /// 源代码行号映射
    line_map: std::collections::HashMap<usize, usize>,
    /// 变量信息
    variables: Vec<VariableInfo>,
    /// 标签位置
    labels: std::collections::HashMap<String, usize>,
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

    fn add_variable(&mut self, name: &str, location: VariableLocation, scope_start: usize, scope_end: usize) {
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

/// 变量位置
#[derive(Debug, Clone)]
pub enum VariableLocation {
    /// 在寄存器中
    Register(u8),
    /// 在栈上（相对于帧指针的偏移）
    Stack(i32),
}

/// 变量信息
#[derive(Debug, Clone)]
pub struct VariableInfo {
    /// 变量名
    pub name: String,
    /// 寄存器或栈偏移
    pub location: VariableLocation,
    /// 生命周期（机器码偏移范围）
    pub scope: (usize, usize),
} 