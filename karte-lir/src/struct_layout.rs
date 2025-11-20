//! 结构体布局管理模块
//!
//! 负责计算结构体的内存布局，包括字段偏移、对齐和大小计算

use crate::{StructField, StructLayout};
use karte_hir::types::{StructField as HirStructField, Type};
use std::collections::HashMap;

/// 结构体布局管理器
#[derive(Debug, Clone)]
pub struct StructLayoutManager {
    /// 已计算的布局缓存
    layout_cache: HashMap<String, StructLayout>,
    /// 基本类型大小表
    type_sizes: HashMap<String, usize>,
    /// 基本类型对齐表
    type_alignments: HashMap<String, usize>,
}

impl StructLayoutManager {
    /// 创建新的布局管理器
    pub fn new() -> Self {
        let mut manager = Self {
            layout_cache: HashMap::new(),
            type_sizes: HashMap::new(),
            type_alignments: HashMap::new(),
        };

        // 初始化基本类型的大小和对齐信息
        manager.init_basic_types();
        manager
    }

    /// 初始化基本类型信息
    fn init_basic_types(&mut self) {
        // 64位平台的标准类型大小
        self.type_sizes.insert("number".to_string(), 8); // i64
        self.type_sizes.insert("boolean".to_string(), 1); // bool
        self.type_sizes.insert("reference".to_string(), 8); // 指针
        self.type_sizes.insert("unit".to_string(), 0); // ()

        // 对齐要求（通常与大小相同，但不超过机器字大小）
        self.type_alignments.insert("number".to_string(), 8);
        self.type_alignments.insert("boolean".to_string(), 1);
        self.type_alignments.insert("reference".to_string(), 8);
        self.type_alignments.insert("unit".to_string(), 1);
    }

    /// 计算结构体布局
    pub fn compute_layout(
        &mut self,
        name: &str,
        fields: &[HirStructField],
    ) -> Result<StructLayout, String> {
        // 检查缓存
        if let Some(cached) = self.layout_cache.get(name) {
            return Ok(cached.clone());
        }

        // 计算布局
        let layout = self.compute_layout_internal(name, fields)?;

        // 缓存结果
        self.layout_cache.insert(name.to_string(), layout.clone());

        Ok(layout)
    }

    /// 内部布局计算逻辑
    fn compute_layout_internal(
        &mut self,
        name: &str,
        fields: &[HirStructField],
    ) -> Result<StructLayout, String> {
        let mut struct_fields = Vec::new();
        let mut current_offset = 0;
        let mut max_alignment = 1;

        for hir_field in fields {
            let (field_size, field_alignment) = self.get_type_info(&hir_field.field_type)?;

            // 更新最大对齐要求
            max_alignment = max_alignment.max(field_alignment);

            // 计算字段对齐偏移
            let aligned_offset = align_up(current_offset, field_alignment);

            let lir_field = StructField {
                name: hir_field.name.clone(),
                offset: aligned_offset,
                size: field_size,
                alignment: field_alignment,
            };

            struct_fields.push(lir_field);
            current_offset = aligned_offset + field_size;
        }

        // 结构体总大小需要对齐到最大字段对齐
        let total_size = align_up(current_offset, max_alignment);

        Ok(StructLayout {
            name: name.to_string(),
            fields: struct_fields,
            total_size,
            alignment: max_alignment,
        })
    }

    /// 获取类型的大小和对齐信息
    fn get_type_info(&mut self, ty: &Type) -> Result<(usize, usize), String> {
        match ty {
            Type::Number => Ok((8, 8)),
            Type::Unit => Ok((0, 1)),
            Type::Reference { inner: _ } => Ok((8, 8)),
            Type::Struct { name, fields } => {
                // 递归计算结构体类型
                let layout = self.compute_layout(name, fields)?;
                Ok((layout.total_size, layout.alignment))
            }
            Type::Function { .. } => Ok((8, 8)), // 函数指针
            Type::Sum { name, .. } => {
                // 对于Sum类型（包括Bool），我们使用固定大小
                if name == "Bool" {
                    Ok((1, 1)) // 布尔值用1字节
                } else {
                    Ok((8, 8)) // 其他Sum类型用8字节（标签+数据）
                }
            }
            Type::Array { .. } => Ok((8, 8)), // 数组值在运行时以指针表示
            Type::Var(_) => Ok((8, 8)),       // 类型变量默认8字节
            Type::Unknown => Ok((8, 8)),      // 未知类型默认8字节
        }
    }

    /// 查找字段信息
    pub fn find_field(&self, struct_name: &str, field_name: &str) -> Option<&StructField> {
        self.layout_cache
            .get(struct_name)?
            .fields
            .iter()
            .find(|field| field.name == field_name)
    }

    /// 获取结构体布局
    pub fn get_layout(&self, name: &str) -> Option<&StructLayout> {
        self.layout_cache.get(name)
    }

    /// 验证结构体布局的一致性
    pub fn validate_layout(&self, layout: &StructLayout) -> Result<(), String> {
        let mut expected_offset = 0;

        for field in &layout.fields {
            // 检查对齐
            if field.offset % field.alignment != 0 {
                return Err(format!(
                    "字段 '{}' 偏移 {} 未按 {} 字节对齐",
                    field.name, field.offset, field.alignment
                ));
            }

            // 检查偏移顺序
            if field.offset < expected_offset {
                return Err(format!(
                    "字段 '{}' 偏移 {} 与预期偏移 {} 冲突",
                    field.name, field.offset, expected_offset
                ));
            }

            expected_offset = field.offset + field.size;
        }

        // 检查总大小对齐
        if layout.total_size % layout.alignment != 0 {
            return Err(format!(
                "结构体 '{}' 总大小 {} 未按 {} 字节对齐",
                layout.name, layout.total_size, layout.alignment
            ));
        }

        Ok(())
    }

    /// 计算结构体的填充开销
    pub fn calculate_padding_overhead(&self, layout: &StructLayout) -> usize {
        let mut field_sizes_sum = 0;
        for field in &layout.fields {
            field_sizes_sum += field.size;
        }
        layout.total_size.saturating_sub(field_sizes_sum)
    }

    /// 分析结构体布局并提供优化建议
    pub fn analyze_layout(&self, layout: &StructLayout) -> LayoutAnalysis {
        let padding_overhead = self.calculate_padding_overhead(layout);
        let field_count = layout.fields.len();
        let avg_field_size = if field_count > 0 {
            layout.fields.iter().map(|f| f.size).sum::<usize>() / field_count
        } else {
            0
        };

        let padding_ratio = if layout.total_size > 0 {
            (padding_overhead as f64) / (layout.total_size as f64)
        } else {
            0.0
        };

        let mut suggestions = Vec::new();

        // 检查填充过多
        if padding_ratio > 0.25 {
            suggestions.push("考虑重新排列字段以减少填充开销".to_string());
        }

        // 检查对齐效率
        if layout.alignment > 8 {
            suggestions.push("结构体对齐要求过高，可能影响内存使用效率".to_string());
        }

        LayoutAnalysis {
            total_size: layout.total_size,
            field_count,
            padding_overhead,
            padding_ratio,
            avg_field_size,
            max_alignment: layout.alignment,
            suggestions,
        }
    }
}

impl Default for StructLayoutManager {
    fn default() -> Self {
        Self::new()
    }
}

/// 结构体布局分析结果
#[derive(Debug, Clone)]
pub struct LayoutAnalysis {
    pub total_size: usize,
    pub field_count: usize,
    pub padding_overhead: usize,
    pub padding_ratio: f64,
    pub avg_field_size: usize,
    pub max_alignment: usize,
    pub suggestions: Vec<String>,
}

/// 对齐辅助函数
fn align_up(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use karte_hir::types::{StructField as HirStructField, Type};

    #[test]
    fn test_basic_struct_layout() {
        let mut manager = StructLayoutManager::new();

        let fields = vec![
            HirStructField {
                name: "x".to_string(),
                field_type: Type::Number,
            },
            HirStructField {
                name: "y".to_string(),
                field_type: Type::Number,
            },
        ];

        let layout = manager.compute_layout("Point", &fields).unwrap();

        assert_eq!(layout.total_size, 16);
        assert_eq!(layout.alignment, 8);
        assert_eq!(layout.fields.len(), 2);
        assert_eq!(layout.fields[0].offset, 0);
        assert_eq!(layout.fields[1].offset, 8);
    }

    #[test]
    fn test_mixed_type_struct() {
        let mut manager = StructLayoutManager::new();

        let fields = vec![
            HirStructField {
                name: "flag".to_string(),
                field_type: Type::bool(),
            },
            HirStructField {
                name: "value".to_string(),
                field_type: Type::Number,
            },
        ];

        let layout = manager.compute_layout("Mixed", &fields).unwrap();

        // bool (1 byte) + 7 bytes padding + i64 (8 bytes) = 16 bytes
        assert_eq!(layout.total_size, 16);
        assert_eq!(layout.alignment, 8);
        assert_eq!(layout.fields[0].offset, 0);
        assert_eq!(layout.fields[1].offset, 8); // 对齐到8字节边界
    }

    #[test]
    fn test_layout_validation() {
        let layout = StructLayout {
            name: "Test".to_string(),
            fields: vec![
                StructField {
                    name: "a".to_string(),
                    offset: 0,
                    size: 1,
                    alignment: 1,
                },
                StructField {
                    name: "b".to_string(),
                    offset: 8,
                    size: 8,
                    alignment: 8,
                },
            ],
            total_size: 16,
            alignment: 8,
        };

        let manager = StructLayoutManager::new();
        assert!(manager.validate_layout(&layout).is_ok());
    }

    #[test]
    fn test_padding_calculation() {
        let layout = StructLayout {
            name: "Test".to_string(),
            fields: vec![
                StructField {
                    name: "flag".to_string(),
                    offset: 0,
                    size: 1,
                    alignment: 1,
                },
                StructField {
                    name: "value".to_string(),
                    offset: 8,
                    size: 8,
                    alignment: 8,
                },
            ],
            total_size: 16,
            alignment: 8,
        };

        let manager = StructLayoutManager::new();
        let padding = manager.calculate_padding_overhead(&layout);
        assert_eq!(padding, 7); // 7 bytes of padding
    }
}
