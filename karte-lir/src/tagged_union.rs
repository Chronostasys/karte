//! Tagged Union 实现系统
//!
//! 这个模块实现了安全的加法类型系统，使用结构体作为tagged union，
//! 避免与用户数据的编码冲突。

use crate::ir::*;
use std::collections::HashMap;

/// Tagged Union的标签类型
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaggedUnionTag {
    /// 类型名称 (如 "Option", "Bool")
    pub type_name: String,
    /// 构造器名称 (如 "Some", "None", "True", "False")
    pub constructor_name: String,
}

impl TaggedUnionTag {
    pub fn new(type_name: String, constructor_name: String) -> Self {
        Self {
            type_name,
            constructor_name,
        }
    }

    /// 为内置的boolean类型创建标签
    pub fn bool_true() -> Self {
        Self::new("Bool".to_string(), "True".to_string())
    }

    pub fn bool_false() -> Self {
        Self::new("Bool".to_string(), "False".to_string())
    }

    /// 为Option类型创建标签
    pub fn option_some() -> Self {
        Self::new("Option".to_string(), "Some".to_string())
    }

    pub fn option_none() -> Self {
        Self::new("Option".to_string(), "None".to_string())
    }
}

/// Tagged Union结构体布局
///
/// 内存布局：
/// ```
/// struct TaggedUnion {
///     tag: i64,        // 8字节 - 标签标识符
///     data: i64,       // 8字节 - 数据载荷（可选）
/// }
/// ```
/// 总大小：16字节，8字节对齐
#[derive(Debug, Clone)]
pub struct TaggedUnionLayout {
    /// 标签字段偏移 (总是0)
    pub tag_offset: usize,
    /// 数据字段偏移 (总是8)
    pub data_offset: usize,
    /// 总大小 (总是16)
    pub total_size: usize,
    /// 对齐要求 (总是8)
    pub alignment: usize,
}

impl Default for TaggedUnionLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl TaggedUnionLayout {
    pub fn new() -> Self {
        Self {
            tag_offset: 0,
            data_offset: 8,
            total_size: 16,
            alignment: 8,
        }
    }
}

/// Tagged Union管理器
///
/// 负责管理所有的tagged union类型，包括：
/// - 标签到数值ID的映射
/// - 结构体布局管理
/// - 构造器和模式匹配支持
pub struct TaggedUnionManager {
    /// 标签到数值ID的映射
    tag_to_id: HashMap<TaggedUnionTag, i64>,
    /// 数值ID到标签的反向映射
    id_to_tag: HashMap<i64, TaggedUnionTag>,
    /// 下一个可用的标签ID
    next_tag_id: i64,
    /// Tagged Union的通用布局
    layout: TaggedUnionLayout,
}

impl TaggedUnionManager {
    pub fn new() -> Self {
        let mut manager = Self {
            tag_to_id: HashMap::new(),
            id_to_tag: HashMap::new(),
            next_tag_id: 1, // 从1开始，0保留为无效标签
            layout: TaggedUnionLayout::new(),
        };

        // 预注册内置类型的标签
        manager.register_builtin_tags();
        manager
    }

    /// 注册内置类型的标签
    fn register_builtin_tags(&mut self) {
        // Boolean类型
        self.register_tag(TaggedUnionTag::bool_true());
        self.register_tag(TaggedUnionTag::bool_false());

        // Option类型
        self.register_tag(TaggedUnionTag::option_some());
        self.register_tag(TaggedUnionTag::option_none());
    }

    /// 注册新的标签
    pub fn register_tag(&mut self, tag: TaggedUnionTag) -> i64 {
        if let Some(&existing_id) = self.tag_to_id.get(&tag) {
            return existing_id;
        }

        let tag_id = self.next_tag_id;
        self.next_tag_id += 1;

        self.tag_to_id.insert(tag.clone(), tag_id);
        self.id_to_tag.insert(tag_id, tag);

        tag_id
    }

    /// 根据标签获取ID
    pub fn get_tag_id(&self, tag: &TaggedUnionTag) -> Option<i64> {
        self.tag_to_id.get(tag).copied()
    }

    /// 根据ID获取标签
    pub fn get_tag(&self, id: i64) -> Option<&TaggedUnionTag> {
        self.id_to_tag.get(&id)
    }

    /// 根据构造器名称获取标签ID（用于简单构造器）
    pub fn get_constructor_id(&mut self, constructor_name: &str) -> i64 {
        match constructor_name {
            "true" | "True" => self.get_tag_id(&TaggedUnionTag::bool_true()).unwrap(),
            "false" | "False" => self.get_tag_id(&TaggedUnionTag::bool_false()).unwrap(),
            "Some" => self.get_tag_id(&TaggedUnionTag::option_some()).unwrap(),
            "None" => self.get_tag_id(&TaggedUnionTag::option_none()).unwrap(),
            _ => {
                // 为用户自定义构造器创建标签
                let tag =
                    TaggedUnionTag::new("UserDefined".to_string(), constructor_name.to_string());
                self.register_tag(tag)
            }
        }
    }

    /// 根据限定构造器获取标签ID
    pub fn get_qualified_constructor_id(&mut self, type_name: &str, constructor_name: &str) -> i64 {
        let tag = TaggedUnionTag::new(type_name.to_string(), constructor_name.to_string());
        self.register_tag(tag)
    }

    /// 获取Tagged Union的布局信息
    pub fn get_layout(&self) -> &TaggedUnionLayout {
        &self.layout
    }

    /// 生成Tagged Union分配指令
    pub fn generate_allocation_instructions(
        &self,
        dst_register: Register,
        tag_id: i64,
        data_value: Option<Operand>,
        span: karte_diagnostics::Span,
    ) -> Vec<Instruction> {
        let mut instructions = Vec::new();

        // 1. 分配Tagged Union结构体内存 (16字节)
        instructions.push(Instruction::Alloc {
            dst: dst_register,
            size: self.layout.total_size,
            alignment: self.layout.alignment,
            allocation_type: AllocationType::Stack,
            span,
        });

        // 2. 存储标签到tag字段 (偏移0)
        instructions.push(Instruction::Store64 {
            addr: dst_register,
            offset: self.layout.tag_offset as i64,
            src: Operand::Immediate { value: tag_id },
            span,
        });

        // 3. 存储数据到data字段 (偏移8)，如果有数据的话
        if let Some(data) = data_value {
            instructions.push(Instruction::Store64 {
                addr: dst_register,
                offset: self.layout.data_offset as i64,
                src: data,
                span,
            });
        } else {
            // 无数据的构造器，存储0作为占位符
            instructions.push(Instruction::Store64 {
                addr: dst_register,
                offset: self.layout.data_offset as i64,
                src: Operand::Immediate { value: 0 },
                span,
            });
        }

        instructions
    }

    /// 生成Tagged Union标签检查指令（用于模式匹配）
    pub fn generate_tag_check_instructions(
        &self,
        union_addr: Register,
        expected_tag_id: i64,
        temp_register: Register,
        span: karte_diagnostics::Span,
    ) -> Vec<Instruction> {
        vec![
            // 从Tagged Union中加载标签
            Instruction::Load64 {
                dst: temp_register,
                addr: union_addr,
                offset: self.layout.tag_offset as i64,
                span,
            },
            // 比较标签是否匹配
            Instruction::Compare {
                src1: Operand::Register { id: temp_register },
                src2: Operand::Immediate {
                    value: expected_tag_id,
                },
                span,
            },
        ]
    }

    /// 生成Tagged Union数据提取指令
    pub fn generate_data_extraction_instructions(
        &self,
        union_addr: Register,
        dst_register: Register,
        span: karte_diagnostics::Span,
    ) -> Vec<Instruction> {
        vec![
            // 从Tagged Union中加载数据
            Instruction::Load64 {
                dst: dst_register,
                addr: union_addr,
                offset: self.layout.data_offset as i64,
                span,
            },
        ]
    }

    /// 显示所有注册的标签（用于调试）
    pub fn debug_print_tags(&self) {
        println!("=== Tagged Union标签注册表 ===");
        for (tag, id) in &self.tag_to_id {
            println!("  {}::{} -> ID {}", tag.type_name, tag.constructor_name, id);
        }
        println!("总共注册了 {} 个标签", self.tag_to_id.len());
    }
}

impl Default for TaggedUnionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karte_diagnostics::Span;

    #[test]
    fn test_tagged_union_manager_creation() {
        let manager = TaggedUnionManager::new();

        // 验证内置标签已注册
        assert!(manager.get_tag_id(&TaggedUnionTag::bool_true()).is_some());
        assert!(manager.get_tag_id(&TaggedUnionTag::bool_false()).is_some());
        assert!(manager.get_tag_id(&TaggedUnionTag::option_some()).is_some());
        assert!(manager.get_tag_id(&TaggedUnionTag::option_none()).is_some());
    }

    #[test]
    fn test_constructor_id_generation() {
        let mut manager = TaggedUnionManager::new();

        // 测试内置构造器
        let true_id = manager.get_constructor_id("True");
        let false_id = manager.get_constructor_id("False");
        let some_id = manager.get_constructor_id("Some");
        let none_id = manager.get_constructor_id("None");

        // 验证ID唯一性
        assert_ne!(true_id, false_id);
        assert_ne!(some_id, none_id);
        assert_ne!(true_id, some_id);

        // 测试用户自定义构造器
        let custom_id = manager.get_constructor_id("CustomConstructor");
        assert_ne!(custom_id, true_id);
    }

    #[test]
    fn test_qualified_constructor_id() {
        let mut manager = TaggedUnionManager::new();

        let color_red = manager.get_qualified_constructor_id("Color", "Red");
        let status_red = manager.get_qualified_constructor_id("Status", "Red");

        // 相同构造器名但不同类型应该有不同ID
        assert_ne!(color_red, status_red);
    }

    #[test]
    fn test_layout_constants() {
        let layout = TaggedUnionLayout::new();

        assert_eq!(layout.tag_offset, 0);
        assert_eq!(layout.data_offset, 8);
        assert_eq!(layout.total_size, 16);
        assert_eq!(layout.alignment, 8);
    }

    #[test]
    fn test_instruction_generation() {
        let manager = TaggedUnionManager::new();
        let dst_reg = Register::Virtual(1);
        let tag_id = manager.get_tag_id(&TaggedUnionTag::bool_true()).unwrap();
        let span = Span::dummy();

        let instructions = manager.generate_allocation_instructions(
            dst_reg, tag_id, None, // 无数据构造器
            span,
        );

        // 应该生成3条指令：Alloc + Store64(tag) + Store64(data=0)
        assert_eq!(instructions.len(), 3);

        // 验证第一条是Alloc指令
        matches!(instructions[0], Instruction::Alloc { .. });

        // 验证第二条是Store64指令（存储标签）
        matches!(instructions[1], Instruction::Store64 { offset: 0, .. });

        // 验证第三条是Store64指令（存储数据）
        matches!(instructions[2], Instruction::Store64 { offset: 8, .. });
    }
}
