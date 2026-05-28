//! LIR降低过程的上下文管理
//!
//! 本模块包含LirLoweringContext的核心管理方法。

use super::helpers::stable_label_from_parts;
use super::types::LirLoweringContext;
use crate::tagged_union::TaggedUnionManager;
use crate::{Instruction, LabelId, LirFunction};
use karte_mir::BasicBlockId;
use std::collections::HashMap;

impl Default for LirLoweringContext {
    fn default() -> Self {
        Self::new()
    }
}

impl LirLoweringContext {
    pub fn new() -> Self {
        Self {
            current_function: None,
            block_to_label: HashMap::new(),
            function_labels: HashMap::new(),
            function_symbols: HashMap::new(),
            current_function_symbol: None,
            current_function_params: vec![],
            label_seed: 0,
            pending_instructions: vec![],
            errors: vec![],
            tagged_union_manager: TaggedUnionManager::new(),
            stack_allocations: HashMap::new(),
            global_struct_types: HashMap::new(),
            struct_value_layouts: HashMap::new(),
            handler_block_param: HashMap::new(),
            known_constants: HashMap::new(),
        }
    }

    pub fn start_function(&mut self, name: String) {
        self.current_function = Some(LirFunction::new(name.clone()));
        // 清理函数相关的状态
        self.pending_instructions.clear();
        // 清空基本块到标签的映射，确保每个函数的标签都是唯一的
        self.block_to_label.clear();
        // 清空栈分配，每个函数都重新开始
        self.stack_allocations.clear();
    }

    pub fn start_function_with_params(&mut self, name: String, params: &[String]) {
        self.current_function = Some(LirFunction::new_with_params(name.clone(), params.len()));
        // 清理函数相关的状态
        self.pending_instructions.clear();
        // 清空基本块到标签的映射，确保每个函数的标签都是唯一的
        self.block_to_label.clear();
        // 清空栈分配，每个函数都重新开始
        self.stack_allocations.clear();
        self.label_seed = 0;
        self.known_constants.clear();
        self.current_function_symbol = self
            .function_symbols
            .get(&name)
            .cloned()
            .or_else(|| Some(name.clone()));

        // 设置当前函数参数列表
        self.current_function_params = params.to_vec();

        log::debug!(
            "🔧 创建函数 {} 包含 {} 个参数: {:?}",
            name,
            params.len(),
            params
        );
    }

    pub fn finish_function(&mut self) -> Option<LirFunction> {
        let mut f = self.current_function.take();
        if let Some(func) = &mut f {
            func.instructions.append(&mut self.pending_instructions);
        }
        self.current_function_symbol = None;
        f
    }

    pub(super) fn current_function_mut(&mut self) -> &mut LirFunction {
        self.current_function.as_mut().expect("No current function")
    }

    pub(super) fn allocate_label_for_block(&mut self, block_id: BasicBlockId) -> LabelId {
        if let Some(label) = self.block_to_label.get(&block_id) {
            return *label;
        }
        let symbol = self
            .current_function_symbol
            .clone()
            .unwrap_or_else(|| "anonymous_function".to_string());
        let label = stable_label_from_parts(&[&symbol, "bb", &block_id.0.to_string()]);
        self.block_to_label.insert(block_id, label);
        label
    }

    pub(super) fn next_internal_label(&mut self, hint: &str) -> LabelId {
        let symbol = self
            .current_function_symbol
            .clone()
            .unwrap_or_else(|| "anonymous_function".to_string());
        let label = stable_label_from_parts(&[&symbol, "gen", hint, &self.label_seed.to_string()]);
        self.label_seed += 1;
        label
    }

    pub(super) fn add_instruction(&mut self, instruction: Instruction) {
        let is_conditional_jmp = match &instruction {
            Instruction::JumpEqual { .. }
            | Instruction::JumpGreater { .. }
            | Instruction::JumpGreaterEqual { .. }
            | Instruction::JumpIndirect { .. }
            | Instruction::JumpLess { .. }
            | Instruction::JumpLessEqual { .. }
            | Instruction::JumpNotEqual { .. } => true,
            _ => false,
        };
        self.current_function_mut().add_instruction(instruction);
        // 如果是条件jmp，则自动在后面插入一个label
        if is_conditional_jmp {
            let label = self.current_function_mut().new_label();
            self.current_function_mut()
                .add_instruction(Instruction::Label {
                    id: label,
                    span: karte_diagnostics::Span::dummy(),
                });
        }
    }
}
