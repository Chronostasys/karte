//! 逃逸点检测器
//!
//! 识别MIR中的逃逸点，即变量从栈语义转换到堆语义的临界点

use crate::{VariableEscapeInfo, VariableId, EscapeState};
use karte_mir::{Statement, Terminator, Value, TempId};
use log::{debug, trace};
use std::collections::HashMap;

/// 逃逸点类型
#[derive(Debug, Clone)]
pub enum EscapePoint {
    /// 取地址操作: %ref = & %var
    AddressOf {
        /// 被取地址的变量
        value: Value,
        /// 语句索引（在基本块中的位置）
        statement_index: usize,
    },
    /// 闭包捕获: 变量被闭包捕获
    ClosureCapture {
        /// 被捕获的变量
        value: Value,
        /// 语句索引
        statement_index: usize,
    },
    /// 返回值: return %var
    Return {
        /// 返回的变量
        value: Value,
    },
}

/// 逃逸点检测器
pub struct EscapePointDetector {
    /// 逃逸分析结果
    escape_info: HashMap<VariableId, VariableEscapeInfo>,

    /// 变量名到ID的映射
    var_name_to_id: HashMap<String, VariableId>,
}

impl EscapePointDetector {
    /// 创建新的逃逸点检测器
    pub fn new(
        escape_info: HashMap<VariableId, VariableEscapeInfo>,
        var_name_to_id: HashMap<String, VariableId>,
    ) -> Self {
        Self {
            escape_info,
            var_name_to_id,
        }
    }

    /// 检查一个Value是否需要在当前位置逃逸到堆
    pub fn needs_escape(&self, value: &Value) -> bool {
        match value {
            Value::Variable { name, .. } => {
                if let Some(&var_id) = self.var_name_to_id.get(name) {
                    if let Some(info) = self.escape_info.get(&var_id) {
                        // 只有 ReturnEscape 或 GlobalEscape 才需要堆分配
                        let needs = matches!(
                            info.escape_state,
                            EscapeState::ReturnEscape | EscapeState::GlobalEscape
                        );
                        if needs {
                            debug!(
                                "变量 {} ({:?}) 需要逃逸: {:?}",
                                name, var_id, info.escape_state
                            );
                        }
                        return needs;
                    }
                }
                false
            }
            Value::Temp { id, .. } => {
                // 根据逃逸分析器的实现，TempId 直接对应 VariableId
                let var_id = VariableId(id.0);
                if let Some(info) = self.escape_info.get(&var_id) {
                    let needs = matches!(
                        info.escape_state,
                        EscapeState::ReturnEscape | EscapeState::GlobalEscape
                    );
                    if needs {
                        debug!(
                            "临时变量 {:?} ({:?}) 需要逃逸: {:?}",
                            id, var_id, info.escape_state
                        );
                    }
                    return needs;
                }
                false
            }
            _ => false,
        }
    }

    /// 识别语句中的逃逸点
    pub fn find_escape_points_in_statement(
        &self,
        stmt: &Statement,
        statement_index: usize,
    ) -> Vec<EscapePoint> {
        match stmt {
            // 类型1: 取地址操作 %ref = & %var
            Statement::Assign {
                source: Value::Reference { value, .. },
                ..
            } => {
                // 检查被取地址的变量是否需要逃逸
                if self.needs_escape(value) {
                    trace!(
                        "发现逃逸点: AddressOf {:?} at statement {}",
                        value,
                        statement_index
                    );
                    vec![EscapePoint::AddressOf {
                        value: (**value).clone(),
                        statement_index,
                    }]
                } else {
                    vec![]
                }
            }

            // 类型2: 闭包捕获
            Statement::Assign {
                source:
                    Value::Closure {
                        captured_values, ..
                    },
                ..
            } => {
                let mut points = Vec::new();
                for captured_value in captured_values {
                    if self.needs_escape(captured_value) {
                        trace!(
                            "发现逃逸点: ClosureCapture {:?} at statement {}",
                            captured_value,
                            statement_index
                        );
                        points.push(EscapePoint::ClosureCapture {
                            value: captured_value.clone(),
                            statement_index,
                        });
                    }
                }
                points
            }

            _ => vec![],
        }
    }

    /// 识别终止器中的逃逸点
    pub fn find_escape_points_in_terminator(&self, terminator: &Terminator) -> Vec<EscapePoint> {
        match terminator {
            // 类型3: 返回语句
            Terminator::Return { value: Some(val), .. } => {
                if self.needs_escape(val) {
                    trace!("发现逃逸点: Return {:?}", val);
                    vec![EscapePoint::Return {
                        value: val.clone(),
                    }]
                } else {
                    vec![]
                }
            }
            _ => vec![],
        }
    }

    /// 获取变量的逃逸信息（用于调试）
    pub fn get_escape_info(&self, var_id: &VariableId) -> Option<&VariableEscapeInfo> {
        self.escape_info.get(var_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AllocationSuggestion, EscapeState, VariableEscapeInfo, VariableId};
    use karte_diagnostics::Span;

    #[test]
    fn test_needs_escape_variable() {
        // 创建测试数据
        let mut escape_info = HashMap::new();
        let var_id = VariableId(1);
        let mut info = VariableEscapeInfo::new_no_escape(var_id);
        info.escape_state = EscapeState::ReturnEscape;
        info.allocation_suggestion = AllocationSuggestion::HeapAlloc;
        escape_info.insert(var_id, info);

        let mut var_name_to_id = HashMap::new();
        var_name_to_id.insert("x".to_string(), var_id);

        let detector = EscapePointDetector::new(escape_info, var_name_to_id);

        // 测试：需要逃逸的变量
        let value = Value::Variable {
            name: "x".to_string(),
            ty: None,
        };
        assert!(detector.needs_escape(&value));

        // 测试：不存在的变量
        let value = Value::Variable {
            name: "y".to_string(),
            ty: None,
        };
        assert!(!detector.needs_escape(&value));
    }

    #[test]
    fn test_find_escape_points_address_of() {
        let mut escape_info = HashMap::new();
        let var_id = VariableId(1);
        let mut info = VariableEscapeInfo::new_no_escape(var_id);
        info.escape_state = EscapeState::ReturnEscape;
        escape_info.insert(var_id, info);

        let mut var_name_to_id = HashMap::new();
        var_name_to_id.insert("d".to_string(), var_id);

        let detector = EscapePointDetector::new(escape_info, var_name_to_id);

        // 创建语句: %ref = & %d
        let stmt = Statement::Assign {
            target: Value::Temp {
                id: TempId(0),
                ty: None,
            },
            source: Value::Reference {
                value: Box::new(Value::Variable {
                    name: "d".to_string(),
                    ty: None,
                }),
                ty: None,
            },
            span: Span::default(),
        };

        let points = detector.find_escape_points_in_statement(&stmt, 0);
        assert_eq!(points.len(), 1);
        assert!(matches!(points[0], EscapePoint::AddressOf { .. }));
    }
}
