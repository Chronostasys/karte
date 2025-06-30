//! 通用指令变换系统
//!
//! 提供统一的指令变换接口，支持基于索引的安全变换操作。
//! 所有 pass 都应该使用这个系统来进行指令的插入、删除和替换操作。

use crate::{Instruction, LirFunction};
use log::{debug, info, trace, warn};

/// 基于索引的变换操作
#[derive(Debug, Clone)]
pub enum IndexTransformOperation {
    Remove(usize),
    Replace(usize, Instruction),
    Insert(usize, Instruction),
}

/// 基于索引的指令变换器
///
/// 提供安全的指令变换操作，自动处理索引偏移问题。
/// 适用于简单的变换场景，如删除、替换、插入指令。
#[derive(Debug, Clone)]
pub struct IndexInstructionTransformer {
    pub transforms: Vec<IndexTransformOperation>,
    pub history: Vec<(usize, IndexTransformOperation)>,
}

impl Default for IndexInstructionTransformer {
    fn default() -> Self {
        Self::new()
    }
}

impl IndexInstructionTransformer {
    pub fn new() -> Self {
        Self {
            transforms: Vec::new(),
            history: Vec::new(),
        }
    }

    /// 添加删除操作
    pub fn remove(&mut self, index: usize) {
        self.transforms.push(IndexTransformOperation::Remove(index));
    }

    /// 添加替换操作
    pub fn replace(&mut self, index: usize, new_instruction: Instruction) {
        self.transforms
            .push(IndexTransformOperation::Replace(index, new_instruction));
    }

    /// 添加插入操作
    pub fn insert(&mut self, index: usize, new_instruction: Instruction) {
        self.transforms
            .push(IndexTransformOperation::Insert(index, new_instruction));
    }

    /// 应用所有变换到函数，自动修正 index 偏移
    pub fn apply_to_function(&mut self, function: &mut LirFunction) -> (bool, usize, usize, usize) {
        if self.transforms.is_empty() {
            return (false, 0, 0, 0);
        }

        info!(
            "🔧 应用 {} 个 index-based 变换，原lir: \n{}",
            self.transforms.len(),
            function
        );
        let mut modified = false;
        let mut removed_count = 0;
        let mut replaced_count = 0;
        let mut inserted_count = 0;

        for op in &self.transforms {
            let mut idx = match op {
                IndexTransformOperation::Remove(i) => *i,
                IndexTransformOperation::Replace(i, _) => *i,
                IndexTransformOperation::Insert(i, _) => *i,
            };

            // 根据历史修正 index 偏移
            for (hist_idx, hist_op) in &self.history {
                if *hist_idx < idx {
                    match hist_op {
                        IndexTransformOperation::Remove(_) => {
                            idx = idx.saturating_sub(1);
                        }
                        IndexTransformOperation::Insert(_, _) => {
                            idx += 1;
                        }
                        _ => {}
                    }
                } else if *hist_idx == idx {
                    // 插入在当前位置，后续操作要向后偏移
                    if let IndexTransformOperation::Insert(_, _) = hist_op {
                        idx += 1;
                    }
                }
            }

            debug!(
                "🔧 应用变换: 原始index={:?}, 修正后index={}",
                match op {
                    IndexTransformOperation::Remove(i) => *i,
                    IndexTransformOperation::Replace(i, _) => *i,
                    IndexTransformOperation::Insert(i, _) => *i,
                },
                idx
            );

            match op {
                IndexTransformOperation::Remove(_) => {
                    if idx < function.instructions.len() {
                        debug!("🔧 删除指令 [{}]: {}", idx, function.instructions[idx]);
                        function.instructions.remove(idx);
                        modified = true;
                        removed_count += 1;
                    }
                }
                IndexTransformOperation::Replace(_, new_instr) => {
                    if idx < function.instructions.len() {
                        debug!(
                            "🔧 替换指令 [{}]: {} -> {}",
                            idx, function.instructions[idx], new_instr
                        );
                        function.instructions[idx] = new_instr.clone();
                        modified = true;
                        replaced_count += 1;
                    }
                }
                IndexTransformOperation::Insert(_, new_instr) => {
                    if idx <= function.instructions.len() {
                        debug!("🔧 插入指令 [{}]: {}", idx, new_instr);
                        function.instructions.insert(idx, new_instr.clone());
                        modified = true;
                        inserted_count += 1;
                    }
                }
            }

            // 记录到历史
            self.history.push((idx, op.clone()));
        }

        self.transforms.clear();
        info!(
            "🔧 index-based 变换完成，修改: {} lir:\n{}",
            modified, function
        );
        (modified, removed_count, replaced_count, inserted_count)
    }

    /// 清空所有待处理的变换
    pub fn clear(&mut self) {
        self.transforms.clear();
    }

    /// 获取待处理变换的数量
    pub fn len(&self) -> usize {
        self.transforms.len()
    }

    /// 检查是否有待处理的变换
    pub fn is_empty(&self) -> bool {
        self.transforms.is_empty()
    }
}

/// 变换历史记录条目
#[derive(Debug, Clone)]
pub struct TransformRecord {
    /// 变换的目标位置（原始位置）
    pub target_position: usize,
    /// 变换类型
    pub transform_type: TransformType,
    /// 变换后的位置变化（对后续指令的影响）
    pub position_delta: i32,
    /// 在序号计数中的位置
    pub sequence_number: usize,
}

/// 变换类型
#[derive(Debug, Clone)]
pub enum TransformType {
    /// 删除指令（position_delta = -1）
    Remove,
    /// 替换指令（position_delta = 0）
    Replace,
    /// 插入指令（position_delta = +1）
    Insert,
}

/// 基于历史的智能变换系统
///
/// 提供更高级的变换功能，包括：
/// - 智能序号修正
/// - 变换历史追踪
/// - 批量操作支持
/// 适用于复杂的变换场景，如跨基本块的变换。
#[derive(Debug, Clone)]
pub struct HistoryBasedTransformer {
    /// 待执行的变换操作
    pub operations: Vec<HistoryBasedOperation>,
    /// 变换历史记录
    pub transform_history: Vec<TransformRecord>,
}

/// 基于历史的变换操作
#[derive(Debug, Clone)]
pub struct HistoryBasedOperation {
    pub operation_type: HistoryBasedOperationType,
    pub original_index: usize,
}

#[derive(Debug, Clone)]
pub enum HistoryBasedOperationType {
    Remove,
    Replace(Instruction),
    Insert(Instruction),
}

impl Default for HistoryBasedTransformer {
    fn default() -> Self {
        Self::new()
    }
}

impl HistoryBasedTransformer {
    pub fn new() -> Self {
        Self {
            operations: Vec::new(),
            transform_history: Vec::new(),
        }
    }

    /// 添加删除操作
    pub fn remove_at(&mut self, index: usize) {
        self.operations.push(HistoryBasedOperation {
            operation_type: HistoryBasedOperationType::Remove,
            original_index: index,
        });
    }

    /// 添加替换操作
    pub fn replace_at(&mut self, index: usize, new_instruction: Instruction) {
        self.operations.push(HistoryBasedOperation {
            operation_type: HistoryBasedOperationType::Replace(new_instruction),
            original_index: index,
        });
    }

    /// 添加插入操作
    pub fn insert_at(&mut self, index: usize, new_instruction: Instruction) {
        self.operations.push(HistoryBasedOperation {
            operation_type: HistoryBasedOperationType::Insert(new_instruction),
            original_index: index,
        });
    }

    /// 应用所有变换到函数（智能序号修正）
    pub fn apply_to_function(&mut self, function: &mut LirFunction) -> (bool, usize, usize, usize) {
        if self.operations.is_empty() {
            return (false, 0, 0, 0);
        }

        info!(
            "🧠 智能变换系统开始：{} 个操作，{} 条历史记录",
            self.operations.len(),
            self.transform_history.len()
        );

        let mut modified = false;
        let mut removed_count = 0;
        let mut replaced_count = 0;
        let mut inserted_count = 0;

        // 逐个应用操作，每次都根据历史记录修正序号
        for operation in &self.operations.clone() {
            let corrected_index = self.calculate_corrected_index(operation.original_index);

            debug!(
                "🧠 操作序号修正：原始序号 {} -> 修正序号 {}",
                operation.original_index, corrected_index
            );

            // 应用单个操作
            if self.apply_single_operation(function, operation, corrected_index) {
                modified = true;
                // 统计变换类型
                match &operation.operation_type {
                    HistoryBasedOperationType::Remove => removed_count += 1,
                    HistoryBasedOperationType::Replace(_) => replaced_count += 1,
                    HistoryBasedOperationType::Insert(_) => inserted_count += 1,
                }
            }
        }

        self.operations.clear(); // 清空已处理的操作
        info!(
            "✅ 智能变换系统完成，历史记录数量：{}",
            self.transform_history.len()
        );
        (modified, removed_count, replaced_count, inserted_count)
    }

    /// 根据变换历史计算修正后的序号
    fn calculate_corrected_index(&self, original_index: usize) -> usize {
        let mut corrected_index = original_index;

        trace!("🧠 开始序号修正：原始序号 {}", original_index);

        // 遍历历史记录，计算对当前序号的影响
        for record in &self.transform_history {
            trace!(
                "🧠   检查历史记录：位置 {}, 序号 {}, 变换类型 {:?}",
                record.target_position,
                record.sequence_number,
                record.transform_type
            );

            if record.target_position < original_index {
                match record.transform_type {
                    TransformType::Remove => {
                        corrected_index = corrected_index.saturating_sub(1);
                        trace!(
                            "🧠     删除影响：序号 {} -> {}",
                            corrected_index + 1,
                            corrected_index
                        );
                    }
                    TransformType::Insert => {
                        corrected_index += 1;
                        trace!(
                            "🧠     插入影响：序号 {} -> {}",
                            corrected_index - 1,
                            corrected_index
                        );
                    }
                    TransformType::Replace => {
                        trace!("🧠     替换操作，无序号影响");
                    }
                }
            }
        }

        trace!("🧠 序号修正完成：{} -> {}", original_index, corrected_index);
        corrected_index
    }

    /// 应用单个操作
    fn apply_single_operation(
        &mut self,
        function: &mut LirFunction,
        operation: &HistoryBasedOperation,
        corrected_index: usize,
    ) -> bool {
        debug!(
            "🧠 应用单个操作：序号 {}, 类型 {:?}",
            corrected_index, operation.operation_type
        );

        if corrected_index >= function.instructions.len() {
            warn!("⚠️ 序号超出范围，跳过操作");
            return false;
        }

        // 记录变换历史
        let transform_type = match &operation.operation_type {
            HistoryBasedOperationType::Remove => TransformType::Remove,
            HistoryBasedOperationType::Replace(_) => TransformType::Replace,
            HistoryBasedOperationType::Insert(_) => TransformType::Insert,
        };

        let record = TransformRecord {
            target_position: corrected_index,
            transform_type: transform_type.clone(),
            position_delta: match transform_type {
                TransformType::Remove => -1,
                TransformType::Replace => 0,
                TransformType::Insert => 1,
            },
            sequence_number: operation.original_index,
        };

        self.transform_history.push(record);

        // 执行实际变换
        match &operation.operation_type {
            HistoryBasedOperationType::Remove => {
                debug!(
                    "🧠   删除指令 [{}]: {:?}",
                    corrected_index, function.instructions[corrected_index]
                );
                function.instructions.remove(corrected_index);

                // 更新后续历史记录中的位置
                self.adjust_history_positions_after_removal(corrected_index);
            }
            HistoryBasedOperationType::Replace(new_instruction) => {
                debug!(
                    "🧠   替换指令 [{}]: {:?} -> {:?}",
                    corrected_index, function.instructions[corrected_index], new_instruction
                );
                function.instructions[corrected_index] = new_instruction.clone();
            }
            HistoryBasedOperationType::Insert(new_instruction) => {
                debug!("🧠   插入指令 [{}]: {:?}", corrected_index, new_instruction);
                function
                    .instructions
                    .insert(corrected_index, new_instruction.clone());
            }
        }

        true
    }

    /// 删除操作后调整历史记录中的位置
    fn adjust_history_positions_after_removal(&mut self, removed_position: usize) {
        for record in &mut self.transform_history {
            if record.target_position > removed_position {
                record.target_position = record.target_position.saturating_sub(1);
            }
        }
    }

    /// 清空所有待处理的操作
    pub fn clear(&mut self) {
        self.operations.clear();
    }

    /// 获取待处理操作的数量
    pub fn len(&self) -> usize {
        self.operations.len()
    }

    /// 检查是否有待处理的操作
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }
}

/// 批量变换结果
#[derive(Debug, Clone)]
pub struct BatchTransformResult {
    /// 是否发生了变换
    pub changed: bool,
    /// 删除的指令数量
    pub removed_count: usize,
    /// 替换的指令数量
    pub replaced_count: usize,
    /// 插入的指令数量
    pub inserted_count: usize,
}

/// 批量变换器
///
/// 提供批量变换功能，支持一次性应用多个变换操作。
/// 适用于需要同时处理多个指令的场景。
#[derive(Debug)]
pub struct BatchTransformer {
    index_transformer: IndexInstructionTransformer,
    history_transformer: HistoryBasedTransformer,
}

impl Default for BatchTransformer {
    fn default() -> Self {
        Self::new()
    }
}

impl BatchTransformer {
    pub fn new() -> Self {
        Self {
            index_transformer: IndexInstructionTransformer::new(),
            history_transformer: HistoryBasedTransformer::new(),
        }
    }

    /// 添加删除操作
    pub fn remove(&mut self, index: usize) {
        self.index_transformer.remove(index);
    }

    /// 添加替换操作
    pub fn replace(&mut self, index: usize, new_instruction: Instruction) {
        self.index_transformer.replace(index, new_instruction);
    }

    /// 添加插入操作
    pub fn insert(&mut self, index: usize, new_instruction: Instruction) {
        self.index_transformer.insert(index, new_instruction);
    }

    /// 添加智能删除操作
    pub fn remove_at(&mut self, index: usize) {
        self.history_transformer.remove_at(index);
    }

    /// 添加智能替换操作
    pub fn replace_at(&mut self, index: usize, new_instruction: Instruction) {
        self.history_transformer.replace_at(index, new_instruction);
    }

    /// 添加智能插入操作
    pub fn insert_at(&mut self, index: usize, new_instruction: Instruction) {
        self.history_transformer.insert_at(index, new_instruction);
    }

    /// 应用所有变换
    pub fn apply_to_function(&mut self, function: &mut LirFunction) -> BatchTransformResult {
        let mut result = BatchTransformResult {
            changed: false,
            removed_count: 0,
            replaced_count: 0,
            inserted_count: 0,
        };

        // 先应用 index-based 变换
        let (index_changed, index_removed_count, index_replaced_count, index_inserted_count) =
            self.index_transformer.apply_to_function(function);
        if index_changed {
            result.changed = true;
            result.removed_count += index_removed_count;
            result.replaced_count += index_replaced_count;
            result.inserted_count += index_inserted_count;
        }

        // 再应用 history-based 变换
        let (
            history_changed,
            history_removed_count,
            history_replaced_count,
            history_inserted_count,
        ) = self.history_transformer.apply_to_function(function);
        if history_changed {
            result.changed = true;
            result.removed_count += history_removed_count;
            result.replaced_count += history_replaced_count;
            result.inserted_count += history_inserted_count;
        }

        result
    }

    /// 清空所有待处理的变换
    pub fn clear(&mut self) {
        self.index_transformer.clear();
        self.history_transformer.clear();
    }

    /// 获取总变换数量
    pub fn len(&self) -> usize {
        self.index_transformer.len() + self.history_transformer.len()
    }

    /// 检查是否有待处理的变换
    pub fn is_empty(&self) -> bool {
        self.index_transformer.is_empty() && self.history_transformer.is_empty()
    }
}

/// 变换工具函数
pub mod utils {
    use super::*;

    /// 安全地删除指令
    pub fn safe_remove_instruction(function: &mut LirFunction, index: usize) -> bool {
        if index < function.instructions.len() {
            function.instructions.remove(index);
            true
        } else {
            false
        }
    }

    /// 安全地替换指令
    pub fn safe_replace_instruction(
        function: &mut LirFunction,
        index: usize,
        new_instruction: Instruction,
    ) -> bool {
        if index < function.instructions.len() {
            function.instructions[index] = new_instruction;
            true
        } else {
            false
        }
    }

    /// 安全地插入指令
    pub fn safe_insert_instruction(
        function: &mut LirFunction,
        index: usize,
        new_instruction: Instruction,
    ) -> bool {
        if index <= function.instructions.len() {
            function.instructions.insert(index, new_instruction);
            true
        } else {
            false
        }
    }

    /// 批量删除指令（从后往前，避免索引问题）
    pub fn batch_remove_instructions(function: &mut LirFunction, indices: &[usize]) -> usize {
        let mut sorted_indices: Vec<usize> = indices.to_vec();
        sorted_indices.sort_by(|a, b| b.cmp(a)); // 逆序排序

        let mut removed_count = 0;
        for &index in &sorted_indices {
            if safe_remove_instruction(function, index) {
                removed_count += 1;
            }
        }

        removed_count
    }

    /// 批量替换指令
    pub fn batch_replace_instructions(
        function: &mut LirFunction,
        replacements: &[(usize, Instruction)],
    ) -> usize {
        let mut replaced_count = 0;
        for &(index, ref new_instruction) in replacements {
            if safe_replace_instruction(function, index, new_instruction.clone()) {
                replaced_count += 1;
            }
        }

        replaced_count
    }

    /// 批量插入指令（从后往前，避免索引问题）
    pub fn batch_insert_instructions(
        function: &mut LirFunction,
        insertions: &[(usize, Instruction)],
    ) -> usize {
        let mut sorted_insertions: Vec<(usize, Instruction)> = insertions.to_vec();
        sorted_insertions.sort_by(|a, b| b.0.cmp(&a.0)); // 按位置逆序排序

        let mut inserted_count = 0;
        for (index, instruction) in sorted_insertions {
            if safe_insert_instruction(function, index, instruction) {
                inserted_count += 1;
            }
        }

        inserted_count
    }
}

#[cfg(test)]
mod tests {
    use karte_common::calling_convention::Register;
    use karte_diagnostics::Span;

    use crate::Operand;

    use super::*;

    #[test]
    fn test_index_based_transformer() {
        info!("🔧 测试 index-based 变换系统");

        let mut transformer = IndexInstructionTransformer::new();

        // 创建一个测试函数
        let mut function = LirFunction::new("test_function".to_string());

        // 添加一些测试指令
        function.instructions.push(Instruction::Move {
            dst: Register::Virtual(1),
            src: Operand::Immediate { value: 42 },
            span: Span { start: 0, end: 0 },
        });

        function.instructions.push(Instruction::Store64 {
            addr: Register::Virtual(2),
            offset: 0,
            src: Operand::Immediate { value: 100 },
            span: Span { start: 0, end: 0 },
        });

        function.instructions.push(Instruction::Load64 {
            dst: Register::Virtual(3),
            addr: Register::Virtual(2),
            offset: 0,
            span: Span { start: 0, end: 0 },
        });

        info!("🔧 原始函数有 {} 条指令", function.instructions.len());

        // 添加变换操作：删除第1个指令，替换第2个指令
        transformer.remove(1);
        transformer.replace(
            2,
            Instruction::Move {
                dst: Register::Virtual(3),
                src: Operand::Immediate { value: 200 },
                span: Span { start: 0, end: 0 },
            },
        );

        // 应用变换
        let (changed, ..) = transformer.apply_to_function(&mut function);

        info!(
            "🔧 应用变换后，函数有 {} 条指令",
            function.instructions.len()
        );
        info!("🔧 变换是否成功: {}", changed);

        // 验证结果
        assert_eq!(function.instructions.len(), 2); // 删除了1个，保留了2个

        // 第一个指令应该是原始的Move
        if let Instruction::Move { dst, src, .. } = &function.instructions[0] {
            assert_eq!(*dst, Register::Virtual(1));
            assert_eq!(*src, Operand::Immediate { value: 42 });
            info!("✅ 第一个指令正确保留");
        } else {
            panic!("第一个指令应该是Move");
        }

        // 第二个指令应该是替换后的Move
        if let Instruction::Move { dst, src, .. } = &function.instructions[1] {
            assert_eq!(*dst, Register::Virtual(3));
            assert_eq!(*src, Operand::Immediate { value: 200 });
            info!("✅ 第二个指令正确替换");
        } else {
            panic!("第二个指令应该是Move");
        }

        info!("🔧 index-based 变换系统测试完成 ✅");
    }

    #[test]
    fn test_history_based_transformer() {
        info!("🧠 测试基于历史的变换系统");

        let mut transformer = HistoryBasedTransformer::new();

        // 创建一个测试函数
        let mut function = LirFunction::new("test_function".to_string());

        // 添加一些测试指令
        for i in 0..5 {
            function.instructions.push(Instruction::Move {
                dst: Register::Virtual(i),
                src: Operand::Immediate { value: i as i64 },
                span: Span { start: 0, end: 0 },
            });
        }

        info!("🧠 原始函数有 {} 条指令", function.instructions.len());

        // 添加变换操作：删除第1个，替换第3个，插入到第2个位置
        transformer.remove_at(1);
        transformer.replace_at(
            3,
            Instruction::Move {
                dst: Register::Virtual(99),
                src: Operand::Immediate { value: 999 },
                span: Span { start: 0, end: 0 },
            },
        );
        transformer.insert_at(
            2,
            Instruction::Move {
                dst: Register::Virtual(88),
                src: Operand::Immediate { value: 888 },
                span: Span { start: 0, end: 0 },
            },
        );

        // 应用变换
        let (changed, _, _, _) = transformer.apply_to_function(&mut function);

        info!(
            "🧠 应用变换后，函数有 {} 条指令",
            function.instructions.len()
        );
        info!("🧠 变换是否成功: {}", changed);

        // 验证结果
        assert_eq!(function.instructions.len(), 5); // 删除了1个，插入了1个，总共5个

        // 验证指令顺序
        if let Instruction::Move { dst, src, .. } = &function.instructions[0] {
            assert_eq!(*dst, Register::Virtual(0));
            assert_eq!(*src, Operand::Immediate { value: 0 });
        }

        if let Instruction::Move { dst, src, .. } = &function.instructions[1] {
            assert_eq!(*dst, Register::Virtual(88));
            assert_eq!(*src, Operand::Immediate { value: 888 });
        }

        if let Instruction::Move { dst, src, .. } = &function.instructions[2] {
            assert_eq!(*dst, Register::Virtual(2));
            assert_eq!(*src, Operand::Immediate { value: 2 });
        }

        if let Instruction::Move { dst, src, .. } = &function.instructions[3] {
            assert_eq!(*dst, Register::Virtual(99));
            assert_eq!(*src, Operand::Immediate { value: 999 });
        }

        if let Instruction::Move { dst, src, .. } = &function.instructions[4] {
            assert_eq!(*dst, Register::Virtual(4));
            assert_eq!(*src, Operand::Immediate { value: 4 });
        }

        info!("🧠 基于历史的变换系统测试完成 ✅");
    }

    #[test]
    fn test_batch_transformer() {
        info!("📦 测试批量变换器");

        let mut transformer = BatchTransformer::new();

        // 创建一个测试函数
        let mut function = LirFunction::new("test_function".to_string());

        // 添加一些测试指令
        for i in 0..6 {
            function.instructions.push(Instruction::Move {
                dst: Register::Virtual(i),
                src: Operand::Immediate { value: i as i64 },
                span: Span { start: 0, end: 0 },
            });
        }

        info!("📦 原始函数有 {} 条指令", function.instructions.len());

        // 添加多种变换操作
        transformer.remove(1); // index-based
        transformer.replace(
            2,
            Instruction::Move {
                dst: Register::Virtual(100),
                src: Operand::Immediate { value: 100 },
                span: Span { start: 0, end: 0 },
            },
        ); // index-based
        transformer.remove_at(4); // history-based
        transformer.insert_at(
            3,
            Instruction::Move {
                dst: Register::Virtual(200),
                src: Operand::Immediate { value: 200 },
                span: Span { start: 0, end: 0 },
            },
        ); // history-based

        // 应用变换
        let result = transformer.apply_to_function(&mut function);

        info!(
            "📦 应用变换后，函数有 {} 条指令",
            function.instructions.len()
        );
        info!("📦 变换结果: {:?}", result);

        // 验证结果
        assert!(result.changed);
        assert_eq!(result.removed_count, 2);
        assert_eq!(result.replaced_count, 1);
        assert_eq!(result.inserted_count, 1);
        assert_eq!(function.instructions.len(), 5); // 删除了2个，插入了1个，总共5个

        info!("📦 批量变换器测试完成 ✅");
    }
}
