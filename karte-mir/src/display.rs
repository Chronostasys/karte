use crate::ir::*;
use std::fmt;

impl fmt::Display for BasicBlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bb{}", self.0)
    }
}

impl fmt::Display for TempId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "_t{}", self.0)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Variable { name } => write!(f, "{}", name),
            Value::Number { value } => write!(f, "{}", value),
            Value::Boolean { value } => write!(f, "{}", if *value { "true" } else { "false" }),
            Value::Unit => write!(f, "()"),
            Value::Temp { id } => write!(f, "{}", id),
            Value::Constructor { name, arg } => {
                if let Some(arg) = arg {
                    write!(f, "{}({})", name, arg)
                } else {
                    write!(f, "{}", name)
                }
            }
            Value::QualifiedConstructor {
                type_name,
                constructor_name,
                arg,
            } => {
                if let Some(arg) = arg {
                    write!(f, "{}::{}({})", type_name, constructor_name, arg)
                } else {
                    write!(f, "{}::{}", type_name, constructor_name)
                }
            }
            Value::Struct { name, fields } => {
                write!(f, "struct {} {{", name)?;
                for (field_name, field_value) in fields {
                    write!(f, " {} = {}", field_name, field_value)?;
                }
                write!(f, " }}")
            }
            Value::Function { name } => write!(f, "fn:{}", name),
            Value::Closure {
                function_name,
                captured_values,
            } => {
                let captured_values_str = captured_values
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "closure:{}:({})", function_name, captured_values_str)
            }
            Value::Reference { value } => write!(f, "&({})", value),
        }
    }
}

impl fmt::Display for BinaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BinaryOperator::Add => write!(f, "+"),
            BinaryOperator::Subtract => write!(f, "-"),
            BinaryOperator::Multiply => write!(f, "*"),
            BinaryOperator::Divide => write!(f, "/"),
            BinaryOperator::Equal => write!(f, "=="),
            BinaryOperator::NotEqual => write!(f, "!="),
            BinaryOperator::LessThan => write!(f, "<"),
            BinaryOperator::LessEqual => write!(f, "<="),
            BinaryOperator::GreaterThan => write!(f, ">"),
            BinaryOperator::GreaterEqual => write!(f, ">="),
            BinaryOperator::And => write!(f, "&&"),
            BinaryOperator::Or => write!(f, "||"),
        }
    }
}

impl fmt::Display for UnaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnaryOperator::Plus => write!(f, "+"),
            UnaryOperator::Minus => write!(f, "-"),
            UnaryOperator::Not => write!(f, "!"),
        }
    }
}

impl fmt::Display for Statement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Statement::Assign { target, source, .. } => write!(f, "{} = {}", target, source),
            Statement::BinaryOp {
                target,
                left,
                op,
                right,
                ..
            } => write!(f, "{} = {} {} {}", target, left, op, right),
            Statement::UnaryOp {
                target,
                op,
                operand,
                ..
            } => write!(f, "{} = {}{}", target, op, operand),
            Statement::Call {
                target,
                function,
                args,
                ..
            } => {
                let args_str = args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", ");
                if let Some(target) = target {
                    write!(f, "{} = call {}({})", target, function, args_str)
                } else {
                    write!(f, "call {}({})", function, args_str)
                }
            }
            Statement::Store { target, value, .. } => write!(f, "store {} -> {}", value, target),
            Statement::FieldAccess {
                target,
                object,
                field,
                ..
            } => write!(f, "{} = {}.{}", target, object, field),
            Statement::Dereference {
                target,
                reference,
                ..
            } => write!(f, "{} = *{}", target, reference),
            Statement::ConstructorArgExtract {
                target,
                constructor,
                arg_index,
                ..
            } => write!(f, "{} = constructor_arg_extract {} {}", target, constructor, arg_index),
            Statement::FieldAssign {
                object,
                field,
                value,
                ..
            } => write!(f, "{}.{} = {}", object, field, value),
            Statement::HeapAlloc {
                target,
                size,
                object_type,
                ..
            } => write!(f, "{} = heap_alloc {} bytes ({})", target, size, object_type),
            Statement::Phi { target, incoming, .. } => {
                write!(f, "{} = phi(", target)?;
                for (i, (block_id, value)) in incoming.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "bb{}: {}", block_id.0, value)?;
                }
                write!(f, ")")
            }
        }
    }
}

impl fmt::Display for Terminator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Terminator::Goto { target, .. } => write!(f, "goto -> {}", target),
            Terminator::Branch {
                condition,
                then_block,
                else_block,
                ..
            } => {
                write!(f, "branch {} ? {} : {}", condition, then_block, else_block)
            }
            Terminator::Return { value, .. } => {
                if let Some(val) = value {
                    write!(f, "return {}", val)
                } else {
                    write!(f, "return")
                }
            }
            Terminator::Match {
                value,
                arms,
                default,
                ..
            } => {
                let arms_str = arms
                    .iter()
                    .map(|arm| format!("{:?} -> {}", arm.pattern, arm.target))
                    .collect::<Vec<_>>()
                    .join(", ");
                if let Some(default) = default {
                    write!(f, "match {} {{ {}, _ -> {} }}", value, arms_str, default)
                } else {
                    write!(f, "match {} {{ {} }}", value, arms_str)
                }
            }
        }
    }
}

impl fmt::Display for MirFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "fn {}({}) {{", self.name, self.params.join(", "))?;
        let mut block_ids: Vec<_> = self.basic_blocks.keys().collect();
        block_ids.sort_by_key(|id| id.0);

        for block_id in block_ids {
            if let Some(block) = self.basic_blocks.get(block_id) {
                writeln!(f, "  {}:", block.id)?;
                for stmt in &block.statements {
                    writeln!(f, "    {}", stmt)?;
                }
                if let Some(terminator) = &block.terminator {
                    writeln!(f, "    {}", terminator)?;
                }
            }
        }
        writeln!(f, "}}")
    }
}

impl fmt::Display for MirProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut function_names: Vec<_> = self.functions.keys().collect();
        function_names.sort();
        for name in function_names {
            if let Some(function) = self.functions.get(name) {
                writeln!(f, "{}", function)?;
            }
        }
        Ok(())
    }
} 