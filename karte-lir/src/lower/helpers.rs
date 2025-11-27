//! LIR降低过程的辅助函数
//!
//! 本模块包含各种工具函数，用于支持降低过程。

use crate::{Instruction, LabelId, Operand, Register};
use karte_mir::Value;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use super::types::LirLoweringContext;

/// 从字符串部分生成稳定的标签ID
pub(super) fn stable_label_from_parts(parts: &[&str]) -> LabelId {
    let mut hasher = DefaultHasher::new();
    for part in parts {
        part.hash(&mut hasher);
        0xFFu8.hash(&mut hasher);
    }
    let mut value = hasher.finish() as usize;
    if value == 0 {
        value = 1;
    } else if value % 2 == 0 {
        value += 1; // 保持非零并减少冲突
    }
    LabelId(value)
}

/// Helper: collect function names from a Value recursively
pub(super) fn collect_function_names_from_value(value: &karte_mir::Value, set: &mut HashSet<String>) {
    match value {
        karte_mir::Value::Function { name } => {
            set.insert(name.clone());
        }
        karte_mir::Value::Struct { fields, .. } => {
            for v in fields.values() {
                collect_function_names_from_value(v, set);
            }
        }
        karte_mir::Value::Reference { value: inner } => {
            collect_function_names_from_value(inner, set);
        }
        karte_mir::Value::Constructor { arg, .. } => {
            if let Some(a) = arg.as_ref() {
                collect_function_names_from_value(a, set);
            }
        }
        karte_mir::Value::QualifiedConstructor { arg, .. } => {
            if let Some(a) = arg.as_ref() {
                collect_function_names_from_value(a, set);
            }
        }
        _ => {}
    }
}

/// Helper: collect function names referenced in a Statement
pub(super) fn collect_function_names_from_statement(stmt: &karte_mir::Statement, set: &mut HashSet<String>) {
    use karte_mir::Statement::*;
    match stmt {
        Assign { target, source, .. } => {
            collect_function_names_from_value(target, set);
            collect_function_names_from_value(source, set);
        }
        BinaryOp {
            target,
            left,
            right,
            ..
        } => {
            collect_function_names_from_value(target, set);
            collect_function_names_from_value(left, set);
            collect_function_names_from_value(right, set);
        }
        UnaryOp {
            target, operand, ..
        } => {
            collect_function_names_from_value(target, set);
            collect_function_names_from_value(operand, set);
        }
        Call {
            target,
            function,
            args,
            ..
        } => {
            if let Some(t) = target {
                collect_function_names_from_value(t, set);
            }
            collect_function_names_from_value(function, set);
            for a in args {
                collect_function_names_from_value(a, set);
            }
        }
        Store { target, value, .. } => {
            collect_function_names_from_value(target, set);
            collect_function_names_from_value(value, set);
        }
        FieldAccess { target, object, .. } => {
            collect_function_names_from_value(target, set);
            collect_function_names_from_value(object, set);
        }
        Dereference {
            target, reference, ..
        } => {
            collect_function_names_from_value(target, set);
            collect_function_names_from_value(reference, set);
        }
        ConstructorArgExtract {
            target,
            constructor,
            ..
        } => {
            collect_function_names_from_value(target, set);
            collect_function_names_from_value(constructor, set);
        }
        FieldAssign { object, value, .. } => {
            collect_function_names_from_value(object, set);
            collect_function_names_from_value(value, set);
        }
        Allocate { target, .. } => {
            collect_function_names_from_value(target, set);
        }
        Deallocate { pointer, .. } => {
            collect_function_names_from_value(pointer, set);
        }
        Retain { value, .. } => {
            collect_function_names_from_value(value, set);
        }
        Release { value, .. } => {
            collect_function_names_from_value(value, set);
        }
        MarkGcRoot { value, .. } => {
            collect_function_names_from_value(value, set);
        }
        WriteBarrier { object, value, .. } => {
            collect_function_names_from_value(object, set);
            collect_function_names_from_value(value, set);
        }
        ReadBarrier { target, object, .. } => {
            collect_function_names_from_value(target, set);
            collect_function_names_from_value(object, set);
        }
        _ => {}
    }
}

/// Helper: collect function names in a Terminator
pub(super) fn collect_function_names_from_terminator(term: &karte_mir::Terminator, set: &mut HashSet<String>) {
    match term {
        karte_mir::Terminator::Return { value, .. } => {
            if let Some(v) = value {
                collect_function_names_from_value(v, set);
            }
        }
        karte_mir::Terminator::Branch { condition, .. } => {
            collect_function_names_from_value(condition, set);
        }
        karte_mir::Terminator::Match {
            value,
            arms,
            default: _,
            ..
        } => {
            collect_function_names_from_value(value, set);
            for _arm in arms {
                // nothing to do for arm
            }
        }
        _ => {}
    }
}

/// 将值转换为字符串键用于映射
pub(super) fn value_to_key(value: &Value) -> String {
    match value {
        Value::Variable { name } => format!("var:{}", name),
        Value::Temp { id } => format!("temp:{}", id.0),
        Value::Function { name } => format!("fn:{}", name),
        Value::Closure {
            function_name,
            captured_values,
        } => {
            let captured_str = captured_values
                .iter()
                .map(value_to_key)
                .collect::<Vec<_>>()
                .join(",");
            format!("closure:{}:({})", function_name, captured_str)
        }
        // Note: This is a simplification. Hash of constructor/struct would be better
        Value::Constructor { name, arg } => format!("ctor:{}({:?})", name, arg),
        Value::QualifiedConstructor {
            type_name,
            constructor_name,
            arg,
        } => format!("qctor:{}::{}({:?})", type_name, constructor_name, arg),
        Value::Number { value } => format!("num:{}", value),
        Value::Boolean { value } => format!("bool:{}", value),
        Value::Unit => "unit".to_string(),
        Value::Struct { name, fields } => {
            let fields_str = fields
                .iter()
                .map(|(k, v)| format!("{}:{}", k, value_to_key(v)))
                .collect::<Vec<_>>()
                .join(",");
            format!("struct:{}({})", name, fields_str)
        }
        Value::Reference { value } => {
            format!("ref:({})", value_to_key(value))
        }
    }
}

impl LirLoweringContext {
    /// 将任意操作数转换为寄存器，必要时插入Move
    pub(super) fn ensure_register_from_operand(
        &mut self,
        operand: Operand,
        span: karte_diagnostics::Span,
    ) -> Register {
        match operand {
            Operand::Register { id } => id,
            other => {
                let temp_reg = self.current_function_mut().new_register();
                self.add_instruction(Instruction::Move {
                    dst: temp_reg,
                    src: other,
                    span,
                });
                temp_reg
            }
        }
    }
}
