use crate::{
    tagged_union::TaggedUnionManager, AllocationType, Instruction, LabelId, LirFunction,
    LirProgram, Operand, Register, StructField, StructLayout,
};
use karte_mir::{
    BasicBlockId, BinaryOperator, MirProgram, Statement, TempId, Terminator, UnaryOperator, Value,
};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

// 模块组织：将大型lower.rs拆分为多个模块
mod context;
mod helpers;
mod memory;
mod stmt;
mod terminator;
mod types;

// 重导出公共API
pub use types::LirLoweringContext;

// 内部使用的导入
use helpers::*;
use stmt::lower_statement;
use terminator::lower_terminator;

pub fn lower_mir_to_lir(mir_program: &MirProgram) -> Result<LirProgram, Vec<String>> {
    let mut context = LirLoweringContext::new();
    let mut lir_program = LirProgram::new();

    // 🔧 专业修复：从MIR传递结构体类型信息到LIR
    for (name, mir_struct_type) in &mir_program.struct_types {
        let lir_fields: Vec<crate::StructField> = mir_struct_type
            .fields
            .iter()
            .enumerate()
            .map(|(index, field)| {
                crate::StructField {
                    name: field.name.clone(),
                    offset: index * 8, // 简化：每个字段8字节，按顺序排列
                    size: 8,
                    alignment: 8,
                }
            })
            .collect();

        let lir_layout = crate::StructLayout {
            name: name.clone(),
            fields: lir_fields,
            total_size: mir_struct_type.fields.len() * 8,
            alignment: 8,
        };

        lir_program.add_global_struct_type(name.clone(), lir_layout.clone());
        context.global_struct_types.insert(name.clone(), lir_layout);
    }

    // 预处理，为所有函数创建入口标签
    // 我们需要先为所有函数分配标签ID，这样在处理函数调用时就能找到它们
    let mut function_names: Vec<_> = mir_program.functions.keys().cloned().collect();
    function_names.sort();

    let mut function_symbol_map = HashMap::new();
    for name in &function_names {
        let symbol = mir_program
            .function_symbol(name)
            .map(|s| s.to_string())
            .unwrap_or_else(|| name.clone());
        let label_id = stable_label_from_parts(&[&symbol]);
        context.function_labels.insert(name.clone(), label_id);
        function_symbol_map.insert(name.clone(), symbol);
        log::debug!("分配函数标签: {} -> {:?}", name, label_id);
    }
    context.function_symbols = function_symbol_map;

    for (alias, symbol) in &mir_program.external_function_symbols {
        let label_id = stable_label_from_parts(&[symbol]);
        // 注册时同时按 alias 和 canonical symbol 注册标签，
        // 以便在LIR降级阶段通过canonical name (e.g. "utils.sub::multiply")
        // 或者通过import alias访问时都能找到对应标签。
        context.function_labels.insert(alias.clone(), label_id);
        context.function_labels.insert(symbol.clone(), label_id);
        log::debug!("注册外部函数标签: {} / {} -> {:?}", alias, symbol, label_id);
    }

    // 额外扫描MIR中的所有Value，确保使用到的外部函数（以canonical name出现）
    // 也被预分配了label。这会捕获直接通过 module::symbol 引用但未通过
    // import alias 声明的符号（例如直接写 `utils.sub::multiply` 的情况）。
    let mut referenced_funcs: HashSet<String> = HashSet::new();
    for mir_fn in mir_program.functions.values() {
        for block in mir_fn.basic_blocks.values() {
            for stmt in &block.statements {
                collect_function_names_from_statement(stmt, &mut referenced_funcs);
            }
            if let Some(term) = &block.terminator {
                collect_function_names_from_terminator(term, &mut referenced_funcs);
            }
        }
    }

    for name in referenced_funcs {
        if !context.function_labels.contains_key(&name) {
            let label_id = stable_label_from_parts(&[&name]);
            context.function_labels.insert(name.clone(), label_id);
            log::debug!("预分配引用函数标签: {} -> {:?}", name, label_id);
        }
    }

    // 转换每个函数
    for name in &function_names {
        let mir_function = mir_program.functions.get(name).unwrap();
        // 🔧 修复：使用带参数信息的函数创建方法
        context.start_function_with_params(name.clone(), &mir_function.params);

        // 使用预分配的入口标签
        let entry_label = context
            .function_labels
            .get(name)
            .cloned()
            .expect("Function label should exist");
        context.add_instruction(Instruction::Label {
            id: entry_label,
            span: karte_diagnostics::Span::new(0, 0), // Dummy span
        });

        // 简化参数处理：直接让参数变量使用调用约定寄存器
        // 不需要额外的move指令，参数变量直接使用r1, r2, r3, r4

        // 🔧 关键修复：预分配所有临时变量的栈槽
        // 这确保了所有临时变量的栈分配都在函数开始处完成
        context.preallocate_temp_slots(mir_function);

        // 预分配所有基本块的标签
        for &block_id in mir_function.basic_blocks.keys() {
            context.allocate_label_for_block(block_id);
        }

        // MIR Phi 节点收集：
        // 在前驱块的 terminator 之前插入 Store64，把 phi incoming 值写入 phi target 的栈地址
        let mut phi_store_map: std::collections::HashMap<BasicBlockId, Vec<(Register, Value)>> = std::collections::HashMap::new();
        for block in mir_function.basic_blocks.values() {
            for statement in &block.statements {
                if let Statement::Phi { target, incoming, .. } = statement {
                    let phi_addr = match context.lower_to_lvalue(target) {
                        Operand::Register { id } => id,
                        _ => continue,
                    };
                    for (pred_block, pred_value) in incoming {
                        phi_store_map.entry(*pred_block).or_default().push((phi_addr, pred_value.clone()));
                    }
                }
            }
        }

        // MIR Phi 节点收集：
        // 在前驱块的 terminator 之前插入 Store64，把 phi incoming 值写入 phi target 的栈地址
        let mut phi_store_map: std::collections::HashMap<BasicBlockId, Vec<(Register, Value)>> = std::collections::HashMap::new();
        for (block_id, block) in &mir_function.basic_blocks {
            for statement in &block.statements {
                if let Statement::Phi {
                    target: phi_target,
                    incoming,
                    ..
                } = statement
                {
                    let phi_addr = match context.lower_to_lvalue(phi_target) {
                        Operand::Register { id } => id,
                        _ => continue,
                    };
                    for (pred_block, pred_value) in incoming {
                        phi_store_map
                            .entry(*pred_block)
                            .or_default()
                            .push((phi_addr, pred_value.clone()));
                    }
                }
            }
        }

        // 转换每个基本块
        // 按ID顺序处理基本块
        let mut block_ids: Vec<_> = mir_function.basic_blocks.keys().copied().collect();
        block_ids.sort_by_key(|id| id.0);

        for block_id in block_ids {
            if let Some(block) = mir_function.basic_blocks.get(&block_id) {
                // 添加基本块标签
                let label = context.allocate_label_for_block(block_id);
                context.add_instruction(Instruction::Label {
                    id: label,
                    span: karte_diagnostics::Span::new(0, 0), // 简化span处理
                });
                // 如果这是一个handler入口块，绑定payload到变量（通过将r1写入变量的栈槽）
                if let Some(param_name) = context.handler_block_param.get(&block_id) {
                    // 把 r1 写入变量 param_name 的栈槽
                    let var_value = Value::Variable {
                        name: param_name.clone(),
                        ty: None,
                    };
                    let var_addr = context.lower_to_lvalue(&var_value);
                    // 确保目标是寄存器地址
                    let addr_reg = match var_addr {
                        Operand::Register { id } => id,
                        _ => context.current_function_mut().new_register(),
                    };
                    context.add_instruction(Instruction::Store64 {
                        addr: addr_reg,
                        offset: 0,
                        src: Operand::Register {
                            id: Register::Physical(
                                karte_common::calling_convention::REG_EFFECT_PAYLOAD,
                            ),
                        }, // r1 (payload)
                        span: karte_diagnostics::Span::dummy(),
                    });
                }

                // 转换基本块中的语句（跳过 Phi 节点）
                for statement in &block.statements {
                    if let Statement::Phi { .. } = statement {
                        continue;
                    }
                    if let Err(errors) = lower_statement(&mut context, statement) {
                        context.errors.extend(errors);
                    }
                }

                // 在 terminator 之前，为后继块的 phi 节点生成 Store64 指令
                if let Some(phi_moves) = phi_store_map.get(&block_id) {
                    for (addr_reg, incoming_value) in phi_moves {
                        let src = context.lower_to_rvalue(incoming_value);
                        context.add_instruction(Instruction::Store64 {
                            addr: *addr_reg,
                            offset: 0,
                            src,
                            span: karte_diagnostics::Span { start: usize::MAX, end: usize::MAX },
                        });
                    }
                }

                // 转换终结语句
                if let Some(terminator) = &block.terminator {
                    if let Err(errors) = lower_terminator(&mut context, terminator) {
                        context.errors.extend(errors);
                    }
                }
            }
        }

        // 完成函数并添加到程序
        if let Some(mut lir_function) = context.finish_function() {
            // Ensure the stored function name is globally unique by using the
            // canonical symbol (module::symbol) when available.
            if let Some(canonical) = context.function_symbols.get(&lir_function.name).cloned() {
                if canonical != lir_function.name {
                    lir_function.name = canonical;
                }
            }
            lir_program.add_function(lir_function);
        }
    }

    // 设置主函数
    if let Some(main_name) = &mir_program.main_function {
        let canonical = mir_program
            .function_symbol(main_name)
            .unwrap_or(main_name.as_str())
            .to_string();
        lir_program.set_main(canonical);
    }

    // 返回未降级的LIR，让优化阶段处理Alloc指令
    if context.errors.is_empty() {
        log::debug!("=== 返回高级LIR (包含Alloc指令，待优化) ===");
        for (name, function) in &lir_program.functions {
            log::debug!(
                "function {} (stack_frame: {}):",
                name,
                function.stack_frame_size
            );
            for instruction in &function.instructions {
                log::debug!("  {:?}", instruction);
            }
        }
        log::debug!("================================================");

        Ok(lir_program)
    } else {
        Err(context.errors)
    }
}

#[cfg(test)]
mod tests {
    use super::lower_mir_to_lir;
    use karte_diagnostics::Span;
    use karte_mir::{MirFunction, MirProgram, Terminator};

    #[test]
    fn lower_uses_canonical_function_symbols() {
        let mut mir_program = MirProgram::new();
        let mut function = MirFunction::new("main".to_string(), vec![]);
        if let Some(entry_block) = function.get_block_mut(function.entry_block) {
            entry_block.set_terminator(Terminator::Return {
                value: None,
                span: Span::new(0, 0),
            });
        }
        mir_program.add_function(function);
        mir_program.set_main("main".to_string());
        mir_program.set_function_symbol("main", "foo.bar::main");

        let lir_program = lower_mir_to_lir(&mir_program).expect("lowering should succeed");

        assert!(lir_program.functions.contains_key("foo.bar::main"));
        assert!(!lir_program.functions.contains_key("main"));
        assert_eq!(lir_program.main_function.as_deref(), Some("foo.bar::main"));
    }
}
