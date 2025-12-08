//! 逃逸点转换器
//!
//! 在逃逸点插入堆分配和复制代码，实现栈变量到堆的转换

use crate::escape_point_detector::{EscapePoint, EscapePointDetector};
use karte_mir::{BasicBlockId, MirFunction, MirProgram, Statement, TempId, Terminator, Value};
use log::{debug, info, trace};

/// 逃逸点转换器
pub struct EscapePointTransformer {
    detector: EscapePointDetector,
    next_temp_id: usize,
}

impl EscapePointTransformer {
    /// 创建新的转换器
    pub fn new(detector: EscapePointDetector) -> Self {
        Self {
            detector,
            next_temp_id: 10000, // 使用较大的初始值避免冲突
        }
    }

    /// 转换整个程序
    pub fn transform_program(&mut self, program: &mut MirProgram) {
        info!(
            "开始逃逸点转换，程序包含 {} 个函数",
            program.functions.len()
        );

        for (func_name, func) in program.functions.iter_mut() {
            debug!("转换函数: {}", func_name);
            self.transform_function(func);
        }

        info!("逃逸点转换完成");
    }

    /// 转换单个函数
    pub fn transform_function(&mut self, func: &mut MirFunction) {
        // 遍历所有基本块
        let block_ids: Vec<BasicBlockId> = func.basic_blocks.keys().copied().collect();

        for block_id in block_ids {
            self.transform_basic_block(func, block_id);
        }
    }

    /// 转换单个基本块
    fn transform_basic_block(&mut self, func: &mut MirFunction, block_id: BasicBlockId) {
        let block = match func.basic_blocks.get(&block_id) {
            Some(b) => b,
            None => return,
        };

        // 收集原始语句和终止器
        let original_statements = block.statements.clone();
        let original_terminator = block.terminator.clone();

        // 创建新的语句列表
        let mut new_statements = Vec::new();

        // 转换每条语句
        for (index, stmt) in original_statements.iter().enumerate() {
            // 检查是否是逃逸点
            let escape_points = self.detector.find_escape_points_in_statement(stmt, index);

            if escape_points.is_empty() {
                // 不是逃逸点，保持原样
                new_statements.push(stmt.clone());
            } else {
                // 是逃逸点，需要转换
                for escape_point in escape_points {
                    self.transform_escape_point(&escape_point, stmt, &mut new_statements);
                }
            }
        }

        // 转换终止器
        let new_terminator = if let Some(ref terminator) = original_terminator {
            let escape_points = self.detector.find_escape_points_in_terminator(terminator);
            if escape_points.is_empty() {
                Some(terminator.clone())
            } else {
                // 需要转换返回语句
                self.transform_return_terminator(terminator, &escape_points, &mut new_statements)
            }
        } else {
            None
        };

        // 更新基本块
        if let Some(block) = func.basic_blocks.get_mut(&block_id) {
            block.statements = new_statements;
            block.terminator = new_terminator;
        }
    }

    /// 转换逃逸点
    fn transform_escape_point(
        &mut self,
        escape_point: &EscapePoint,
        original_stmt: &Statement,
        new_statements: &mut Vec<Statement>,
    ) {
        match escape_point {
            EscapePoint::AddressOf { value, .. } => {
                // 转换: %ref = & %var
                // 变为: %heap = HeapAlloc(8)
                //       Store { %heap, %var }
                //       %ref = %heap
                self.transform_address_of(value, original_stmt, new_statements);
            }
            EscapePoint::ClosureCapture { value, .. } => {
                // 转换闭包捕获
                self.transform_closure_capture(value, original_stmt, new_statements);
            }
            _ => {
                // 其他类型的逃逸点在别处处理
                new_statements.push(original_stmt.clone());
            }
        }
    }

    /// 转换取地址操作
    /// 原始: %ref = & %var
    /// 转换为: %heap = HeapAlloc(8)
    ///        Store { %heap, %var }
    ///        %ref = & %heap
    ///
    /// 注意：保留 Reference 结构，但引用指向堆地址
    fn transform_address_of(
        &mut self,
        value: &Value,
        original_stmt: &Statement,
        new_statements: &mut Vec<Statement>,
    ) {
        if let Statement::Assign { target, span, .. } = original_stmt {
            debug!("转换取地址操作: {:?} = & {:?}", target, value);

            // 1. 分配堆内存
            let heap_temp = self.alloc_temp();
            new_statements.push(Statement::HeapAlloc {
                target: heap_temp.clone(),
                size: 8,
                object_type: "escaped_value".to_string(),
                span: *span,
            });
            trace!("  插入 HeapAlloc: {:?}", heap_temp);

            // 2. 复制栈值到堆
            new_statements.push(Statement::Store {
                target: heap_temp.clone(),
                value: value.clone(),
                span: *span,
            });
            trace!("  插入 Store: {:?} <- {:?}", heap_temp, value);

            // 3. 创建指向堆地址的引用
            // 注意：这里 heap_temp 已经是堆地址，Reference 只是语义标记
            trace!("  插入 Assign: {:?} = & {:?}", target, heap_temp);
            new_statements.push(Statement::Assign {
                target: target.clone(),
                source: Value::Reference {
                    value: Box::new(heap_temp),
                    ty: None,
                },
                span: *span,
            });
        } else {
            // 不应该到这里
            new_statements.push(original_stmt.clone());
        }
    }

    /// 转换闭包捕获
    /// 原始: %closure = Closure { function_name, captured_values: [%var1, %var2] }
    /// 转换为: %heap1 = HeapAlloc(8)
    ///        Store { %heap1, %var1 }
    ///        %heap2 = HeapAlloc(8)
    ///        Store { %heap2, %var2 }
    ///        %closure = Closure { function_name, captured_values: [%heap1, %heap2] }
    fn transform_closure_capture(
        &mut self,
        escaped_value: &Value,
        original_stmt: &Statement,
        new_statements: &mut Vec<Statement>,
    ) {
        if let Statement::Assign {
            target,
            source:
                Value::Closure {
                    function_name,
                    captured_values,
                    ty,
                },
            span,
        } = original_stmt
        {
            debug!(
                "转换闭包捕获: {:?} 捕获了逃逸变量 {:?}",
                function_name, escaped_value
            );

            // 为每个需要逃逸的捕获变量生成堆分配代码
            let mut new_captured_values = Vec::new();

            for captured_value in captured_values {
                if self.detector.needs_escape(captured_value) {
                    // 需要逃逸：分配堆内存并复制
                    let heap_temp = self.alloc_temp();

                    new_statements.push(Statement::HeapAlloc {
                        target: heap_temp.clone(),
                        size: 8,
                        object_type: "captured_value".to_string(),
                        span: *span,
                    });
                    trace!("  插入 HeapAlloc for 捕获变量: {:?}", heap_temp);

                    new_statements.push(Statement::Store {
                        target: heap_temp.clone(),
                        value: captured_value.clone(),
                        span: *span,
                    });
                    trace!("  插入 Store: {:?} <- {:?}", heap_temp, captured_value);

                    new_captured_values.push(heap_temp);
                } else {
                    // 不需要逃逸：直接使用原值
                    new_captured_values.push(captured_value.clone());
                }
            }

            // 创建新的闭包赋值语句
            new_statements.push(Statement::Assign {
                target: target.clone(),
                source: Value::Closure {
                    function_name: function_name.clone(),
                    captured_values: new_captured_values,
                    ty: ty.clone(),
                },
                span: *span,
            });
            trace!("  插入更新后的 Closure 赋值");
        } else {
            // 不应该到这里
            new_statements.push(original_stmt.clone());
        }
    }

    /// 转换返回语句
    /// 根据设计文档规则3：
    /// - 只有当返回值是"栈变量"且"不是简单值类型"时才转换
    /// - 简单值类型（number, bool）不需要堆分配
    /// - 已经是指针/引用的值不需要再次转换
    fn transform_return_terminator(
        &mut self,
        original_terminator: &Terminator,
        _escape_points: &[EscapePoint],
        _new_statements: &mut Vec<Statement>,
    ) -> Option<Terminator> {
        // 按照设计文档：返回值转换应该在取地址点完成
        // 这里不做转换，直接返回原terminator
        //
        // 原因：
        // 1. 如果返回 &d，那么 & 操作本身就是逃逸点，已经转换过了
        // 2. 如果返回简单值（number），不需要堆分配
        // 3. 避免双重堆分配的问题
        Some(original_terminator.clone())
    }

    /// 分配一个新的临时变量
    fn alloc_temp(&mut self) -> Value {
        let id = TempId(self.next_temp_id);
        self.next_temp_id += 1;
        Value::Temp { id, ty: None }
    }
}
