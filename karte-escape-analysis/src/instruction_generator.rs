//! 分配指令生成器
//!
//! 将分配策略转换为 MIR 指令

use crate::allocation_strategy::{AllocationStatistics, AllocationStrategy};
use crate::types::{VariableEscapeInfo, VariableId};
use std::collections::HashMap;

/// 分配指令生成器
pub struct InstructionGenerator {
    /// 逃逸分析结果
    escape_info: HashMap<VariableId, VariableEscapeInfo>,

    /// 变量名到ID的映射
    var_name_to_id: HashMap<String, VariableId>,

    /// 统计信息
    statistics: AllocationStatistics,
}

impl InstructionGenerator {
    /// 创建新的指令生成器
    pub fn new(
        escape_info: HashMap<VariableId, VariableEscapeInfo>,
        var_name_to_id: HashMap<String, VariableId>,
    ) -> Self {
        Self {
            escape_info,
            var_name_to_id,
            statistics: AllocationStatistics::default(),
        }
    }

    /// 为变量生成分配指令信息
    pub fn generate_allocation_instruction(
        &self,
        var_name: &str,
        strategy: &AllocationStrategy,
    ) -> AllocationInstruction {
        match strategy {
            AllocationStrategy::Stack {
                offset,
                size,
                alignment,
            } => AllocationInstruction::StackAlloc {
                var_name: var_name.to_string(),
                offset: *offset,
                size: *size,
                alignment: *alignment,
            },

            AllocationStrategy::Heap {
                object_type,
                size,
                gc_tracked,
            } => AllocationInstruction::HeapAlloc {
                var_name: var_name.to_string(),
                object_type: object_type.clone(),
                size: *size,
                gc_tracked: *gc_tracked,
            },

            AllocationStrategy::Inline { size } => AllocationInstruction::InlineAlloc {
                var_name: var_name.to_string(),
                size: *size,
            },

            AllocationStrategy::Register {
                register_hint,
                size,
            } => AllocationInstruction::RegisterAlloc {
                var_name: var_name.to_string(),
                register_hint: *register_hint,
                size: *size,
            },
        }
    }

    /// 为所有变量生成分配指令
    pub fn generate_all_instructions(
        &mut self,
        strategies: &HashMap<VariableId, AllocationStrategy>,
    ) -> Vec<AllocationInstruction> {
        let mut instructions = Vec::new();

        for (var_id, strategy) in strategies {
            // 查找变量名
            if let Some((var_name, _)) = self
                .var_name_to_id
                .iter()
                .find(|(_, id)| *id == var_id)
            {
                let instruction = self.generate_allocation_instruction(var_name, strategy);
                instructions.push(instruction);

                // 更新统计
                self.update_statistics(strategy);
            }
        }

        instructions
    }

    /// 更新统计信息
    fn update_statistics(&mut self, strategy: &AllocationStrategy) {
        self.statistics.total_variables += 1;

        match strategy {
            AllocationStrategy::Stack { .. } => {
                self.statistics.stack_allocated += 1;
                self.statistics.suggested_stack += 1;
            }
            AllocationStrategy::Heap { .. } => {
                self.statistics.heap_allocated += 1;
                self.statistics.suggested_heap += 1;
            }
            AllocationStrategy::Inline { .. } => {
                self.statistics.suggested_inline += 1;
            }
            AllocationStrategy::Register { .. } => {
                self.statistics.suggested_register += 1;
            }
        }
    }

    /// 获取统计信息
    pub fn statistics(&self) -> &AllocationStatistics {
        &self.statistics
    }

    /// 打印生成的指令
    pub fn print_instructions(&self, instructions: &[AllocationInstruction]) {
        println!("=== 生成的分配指令 ===");
        for instruction in instructions {
            match instruction {
                AllocationInstruction::StackAlloc {
                    var_name,
                    offset,
                    size,
                    alignment,
                } => {
                    println!(
                        "stackalloc {} (offset={}, size={}, align={})",
                        var_name, offset, size, alignment
                    );
                }
                AllocationInstruction::HeapAlloc {
                    var_name,
                    object_type,
                    size,
                    gc_tracked,
                } => {
                    println!(
                        "heapalloc {} (type={}, size={}, gc={})",
                        var_name, object_type, size, gc_tracked
                    );
                }
                AllocationInstruction::InlineAlloc { var_name, size } => {
                    println!("inlinealloc {} (size={})", var_name, size);
                }
                AllocationInstruction::RegisterAlloc {
                    var_name,
                    register_hint,
                    size,
                } => {
                    println!(
                        "regalloc {} (hint={:?}, size={})",
                        var_name, register_hint, size
                    );
                }
            }
        }
    }
}

/// 分配指令
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllocationInstruction {
    /// 栈分配指令
    StackAlloc {
        var_name: String,
        offset: isize,
        size: usize,
        alignment: usize,
    },

    /// 堆分配指令
    HeapAlloc {
        var_name: String,
        object_type: String,
        size: usize,
        gc_tracked: bool,
    },

    /// 内联分配指令
    InlineAlloc {
        var_name: String,
        size: usize,
    },

    /// 寄存器分配指令
    RegisterAlloc {
        var_name: String,
        register_hint: Option<usize>,
        size: usize,
    },
}

impl AllocationInstruction {
    /// 获取变量名
    pub fn var_name(&self) -> &str {
        match self {
            AllocationInstruction::StackAlloc { var_name, .. } => var_name,
            AllocationInstruction::HeapAlloc { var_name, .. } => var_name,
            AllocationInstruction::InlineAlloc { var_name, .. } => var_name,
            AllocationInstruction::RegisterAlloc { var_name, .. } => var_name,
        }
    }

    /// 检查是否为栈分配
    pub fn is_stack_alloc(&self) -> bool {
        matches!(self, AllocationInstruction::StackAlloc { .. })
    }

    /// 检查是否为堆分配
    pub fn is_heap_alloc(&self) -> bool {
        matches!(self, AllocationInstruction::HeapAlloc { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AllocationSuggestion, EscapeState};

    #[test]
    fn test_instruction_generation() {
        let mut escape_info = HashMap::new();
        let mut var_name_to_id = HashMap::new();

        let var_id = VariableId(0);
        var_name_to_id.insert("x".to_string(), var_id);

        escape_info.insert(
            var_id,
            VariableEscapeInfo {
                variable_id: var_id,
                escape_state: EscapeState::NoEscape,
                escape_points: Vec::new(),
                lifetime_constraints: Vec::new(),
                allocation_suggestion: AllocationSuggestion::StackAlloc,
                depends_on: Default::default(),
                depended_by: Default::default(),
            },
        );

        let generator = InstructionGenerator::new(escape_info, var_name_to_id);

        let strategy = AllocationStrategy::Stack {
            offset: -8,
            size: 8,
            alignment: 8,
        };

        let instruction = generator.generate_allocation_instruction("x", &strategy);

        match instruction {
            AllocationInstruction::StackAlloc {
                var_name,
                offset,
                size,
                alignment,
            } => {
                assert_eq!(var_name, "x");
                assert_eq!(offset, -8);
                assert_eq!(size, 8);
                assert_eq!(alignment, 8);
            }
            _ => panic!("Expected StackAlloc instruction"),
        }
    }

    #[test]
    fn test_heap_instruction_generation() {
        let generator = InstructionGenerator::new(HashMap::new(), HashMap::new());

        let strategy = AllocationStrategy::Heap {
            object_type: "test::obj".to_string(),
            size: 16,
            gc_tracked: true,
        };

        let instruction = generator.generate_allocation_instruction("y", &strategy);

        match instruction {
            AllocationInstruction::HeapAlloc {
                var_name,
                object_type,
                size,
                gc_tracked,
            } => {
                assert_eq!(var_name, "y");
                assert_eq!(object_type, "test::obj");
                assert_eq!(size, 16);
                assert!(gc_tracked);
            }
            _ => panic!("Expected HeapAlloc instruction"),
        }
    }

    #[test]
    fn test_generate_all_instructions() {
        let mut escape_info = HashMap::new();
        let mut var_name_to_id = HashMap::new();

        // 添加两个变量
        let var1 = VariableId(0);
        let var2 = VariableId(1);

        var_name_to_id.insert("x".to_string(), var1);
        var_name_to_id.insert("y".to_string(), var2);

        escape_info.insert(
            var1,
            VariableEscapeInfo {
                variable_id: var1,
                escape_state: EscapeState::NoEscape,
                escape_points: Vec::new(),
                lifetime_constraints: Vec::new(),
                allocation_suggestion: AllocationSuggestion::StackAlloc,
                depends_on: Default::default(),
                depended_by: Default::default(),
            },
        );

        escape_info.insert(
            var2,
            VariableEscapeInfo {
                variable_id: var2,
                escape_state: EscapeState::ReturnEscape,
                escape_points: Vec::new(),
                lifetime_constraints: Vec::new(),
                allocation_suggestion: AllocationSuggestion::HeapAlloc,
                depends_on: Default::default(),
                depended_by: Default::default(),
            },
        );

        let mut generator = InstructionGenerator::new(escape_info, var_name_to_id);

        let mut strategies = HashMap::new();
        strategies.insert(
            var1,
            AllocationStrategy::Stack {
                offset: -8,
                size: 8,
                alignment: 8,
            },
        );
        strategies.insert(
            var2,
            AllocationStrategy::Heap {
                object_type: "test::obj".to_string(),
                size: 16,
                gc_tracked: true,
            },
        );

        let instructions = generator.generate_all_instructions(&strategies);

        assert_eq!(instructions.len(), 2);

        // 检查统计
        let stats = generator.statistics();
        assert_eq!(stats.total_variables, 2);
        assert_eq!(stats.stack_allocated, 1);
        assert_eq!(stats.heap_allocated, 1);
    }

    #[test]
    fn test_instruction_helpers() {
        let stack_instr = AllocationInstruction::StackAlloc {
            var_name: "x".to_string(),
            offset: -8,
            size: 8,
            alignment: 8,
        };

        let heap_instr = AllocationInstruction::HeapAlloc {
            var_name: "y".to_string(),
            object_type: "test::obj".to_string(),
            size: 16,
            gc_tracked: true,
        };

        assert_eq!(stack_instr.var_name(), "x");
        assert_eq!(heap_instr.var_name(), "y");

        assert!(stack_instr.is_stack_alloc());
        assert!(!stack_instr.is_heap_alloc());

        assert!(heap_instr.is_heap_alloc());
        assert!(!heap_instr.is_stack_alloc());
    }
}
