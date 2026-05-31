//! 函数逃逸摘要
//!
//! 该模块实现函数签名级别的逃逸分析摘要，用于过程间分析
//! 采用Go编译器的parameter tags策略，避免全程序调用图分析

use crate::types::{EscapeState, FunctionId};
use std::collections::HashMap;

/// 参数的逃逸行为标签
///
/// 用于标记函数参数在函数体内的使用方式，支持精确的跨函数逃逸分析
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParameterTag {
    /// 参数不逃逸（只读使用，不保存）
    ///
    /// 示例：
    /// ```karte
    /// fn read_value(ptr: &number) -> number {
    ///     *ptr  // 只读取，不保存ptr
    /// }
    /// ```
    NoEscape,

    /// 参数通过返回值逃逸
    ///
    /// 示例：
    /// ```karte
    /// fn identity(ptr: &number) -> &number {
    ///     ptr  // 直接返回参数
    /// }
    /// ```
    EscapeViaReturn,

    /// 参数存储到堆对象中
    ///
    /// 示例：
    /// ```karte
    /// fn store_global(ptr: &number) {
    ///     global_ptr = ptr  // 存储到全局变量
    /// }
    /// ```
    EscapeViaHeapStore,

    /// 参数传递给其他函数
    ///
    /// `callee`: 被调用函数的ID
    /// `param_index`: 参数在被调用函数中的索引
    ///
    /// 示例：
    /// ```karte
    /// fn caller(ptr: &number) {
    ///     callee(ptr)  // 传递给callee函数
    /// }
    /// ```
    EscapeViaCall {
        callee: FunctionId,
        param_index: usize,
    },

    /// 未知行为（保守策略）
    ///
    /// 用于外部函数、递归函数等无法精确分析的情况
    Unknown,
}

impl ParameterTag {
    /// 判断参数是否逃逸
    pub fn escapes(&self) -> bool {
        !matches!(self, ParameterTag::NoEscape)
    }

    /// 获取对应的逃逸状态
    pub fn to_escape_state(&self) -> EscapeState {
        match self {
            ParameterTag::NoEscape => EscapeState::NoEscape,
            ParameterTag::EscapeViaReturn => EscapeState::ReturnEscape,
            ParameterTag::EscapeViaHeapStore => EscapeState::GlobalEscape,
            ParameterTag::EscapeViaCall { .. } => EscapeState::ArgEscape,
            ParameterTag::Unknown => EscapeState::GlobalEscape,
        }
    }

    /// 合并两个参数标签（取更严格的）
    pub fn merge(&self, other: &ParameterTag) -> ParameterTag {
        match (self, other) {
            // Unknown 和 GlobalEscape 最严格
            (ParameterTag::Unknown, _) | (_, ParameterTag::Unknown) => ParameterTag::Unknown,
            (ParameterTag::EscapeViaHeapStore, _) | (_, ParameterTag::EscapeViaHeapStore) => {
                ParameterTag::EscapeViaHeapStore
            }
            (ParameterTag::EscapeViaReturn, _) | (_, ParameterTag::EscapeViaReturn) => {
                ParameterTag::EscapeViaReturn
            }
            (ParameterTag::EscapeViaCall { .. }, _) | (_, ParameterTag::EscapeViaCall { .. }) => {
                // 简化：传递给多个函数时，保守地标记为Unknown
                ParameterTag::Unknown
            }
            _ => ParameterTag::NoEscape,
        }
    }
}

/// 返回值的来源
///
/// 用于追踪函数返回值的数据流来源
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReturnSource {
    /// 返回某个参数
    ///
    /// 示例：
    /// ```karte
    /// fn get_first(a: &number, b: &number) -> &number {
    ///     a  // 返回第0个参数
    /// }
    /// ```
    FromParameter { param_index: usize },

    /// 返回本地分配的值
    ///
    /// 示例：
    /// ```karte
    /// fn create_ptr() -> &number {
    ///     let x = 42;
    ///     &x  // 返回本地变量的引用（逃逸！）
    /// }
    /// ```
    LocalAllocation,

    /// 返回全局数据
    ///
    /// 示例：
    /// ```karte
    /// fn get_global() -> &number {
    ///     &GLOBAL_VALUE
    /// }
    /// ```
    GlobalData,

    /// 返回常量或无返回值
    Constant,

    /// 未知来源（保守）
    Unknown,
}

/// 函数逃逸摘要
///
/// 存储一个函数的逃逸分析结果，用于跨函数分析
#[derive(Debug, Clone)]
pub struct FunctionSummary {
    /// 函数ID
    pub function_id: FunctionId,

    /// 参数标签列表
    ///
    /// `parameter_tags[i]` 表示第i个参数的逃逸行为
    pub parameter_tags: Vec<ParameterTag>,

    /// 返回值来源
    pub return_source: ReturnSource,

    /// 是否修改全局状态
    ///
    /// 包括：修改全局变量、调用有副作用的函数等
    pub modifies_global_state: bool,

    /// 是否是递归函数
    pub is_recursive: bool,
}

impl FunctionSummary {
    /// 创建新的函数摘要
    pub fn new(function_id: FunctionId, param_count: usize) -> Self {
        Self {
            function_id,
            parameter_tags: vec![ParameterTag::NoEscape; param_count],
            return_source: ReturnSource::Constant,
            modifies_global_state: false,
            is_recursive: false,
        }
    }

    /// 创建保守的函数摘要（所有参数都逃逸）
    pub fn conservative(function_id: FunctionId, param_count: usize) -> Self {
        Self {
            function_id,
            parameter_tags: vec![ParameterTag::Unknown; param_count],
            return_source: ReturnSource::Unknown,
            modifies_global_state: true,
            is_recursive: false,
        }
    }

    /// 标记参数逃逸
    pub fn mark_parameter_escape(&mut self, param_index: usize, tag: ParameterTag) {
        if param_index < self.parameter_tags.len() {
            self.parameter_tags[param_index] = self.parameter_tags[param_index].merge(&tag);
        }
    }

    /// 检查参数是否逃逸
    pub fn parameter_escapes(&self, param_index: usize) -> bool {
        self.parameter_tags
            .get(param_index)
            .map(|tag| tag.escapes())
            .unwrap_or(true) // 保守：索引越界时认为逃逸
    }

    /// 获取参数的逃逸状态
    pub fn get_parameter_escape_state(&self, param_index: usize) -> EscapeState {
        self.parameter_tags
            .get(param_index)
            .map(|tag| tag.to_escape_state())
            .unwrap_or(EscapeState::GlobalEscape)
    }
}

/// 函数摘要数据库
///
/// 管理所有函数的逃逸分析摘要
#[derive(Debug, Default)]
pub struct FunctionSummaryDatabase {
    /// 用户函数摘要
    summaries: HashMap<FunctionId, FunctionSummary>,

    /// 内置函数摘要
    builtin_summaries: HashMap<String, FunctionSummary>,
}

impl FunctionSummaryDatabase {
    /// 创建新的函数摘要数据库
    pub fn new() -> Self {
        let mut db = Self {
            summaries: HashMap::new(),
            builtin_summaries: HashMap::new(),
        };

        // 初始化内置函数摘要
        db.init_builtin_summaries();

        db
    }

    /// 初始化内置函数摘要
    fn init_builtin_summaries(&mut self) {
        // print: 不保存参数
        self.builtin_summaries.insert(
            "print".to_string(),
            FunctionSummary {
                function_id: FunctionId("print".to_string()),
                parameter_tags: vec![ParameterTag::NoEscape],
                return_source: ReturnSource::Constant,
                modifies_global_state: true, // 有IO副作用
                is_recursive: false,
            },
        );

        // assert: 不保存参数
        self.builtin_summaries.insert(
            "assert".to_string(),
            FunctionSummary {
                function_id: FunctionId("assert".to_string()),
                parameter_tags: vec![ParameterTag::NoEscape],
                return_source: ReturnSource::Constant,
                modifies_global_state: false,
                is_recursive: false,
            },
        );

        // __runtime_string_char_at: 不保存参数，返回新分配
        self.builtin_summaries.insert(
            "__runtime_string_char_at".to_string(),
            FunctionSummary {
                function_id: FunctionId("__runtime_string_char_at".to_string()),
                parameter_tags: vec![ParameterTag::NoEscape, ParameterTag::NoEscape],
                return_source: ReturnSource::LocalAllocation,
                modifies_global_state: false,
                is_recursive: false,
            },
        );

        // __runtime_string_substring: 不保存参数，返回新分配
        self.builtin_summaries.insert(
            "__runtime_string_substring".to_string(),
            FunctionSummary {
                function_id: FunctionId("__runtime_string_substring".to_string()),
                parameter_tags: vec![ParameterTag::NoEscape, ParameterTag::NoEscape, ParameterTag::NoEscape],
                return_source: ReturnSource::LocalAllocation,
                modifies_global_state: false,
                is_recursive: false,
            },
        );

        // __runtime_string_contains: 不保存参数，不分配内存，返回数值
        self.builtin_summaries.insert(
            "__runtime_string_contains".to_string(),
            FunctionSummary {
                function_id: FunctionId("__runtime_string_contains".to_string()),
                parameter_tags: vec![ParameterTag::NoEscape, ParameterTag::NoEscape],
                return_source: ReturnSource::Constant,
                modifies_global_state: false,
                is_recursive: false,
            },
        );

        // 未来可以添加更多内置函数...
    }

    /// 添加或更新函数摘要
    pub fn insert(&mut self, summary: FunctionSummary) {
        self.summaries.insert(summary.function_id.clone(), summary);
    }

    /// 获取函数摘要
    pub fn get(&self, function_id: &FunctionId) -> Option<&FunctionSummary> {
        // 优先查找用户函数
        if let Some(summary) = self.summaries.get(function_id) {
            return Some(summary);
        }

        // 然后查找内置函数
        self.builtin_summaries.get(&function_id.0)
    }

    /// 获取可变引用
    pub fn get_mut(&mut self, function_id: &FunctionId) -> Option<&mut FunctionSummary> {
        self.summaries.get_mut(function_id)
    }

    /// 检查函数是否存在摘要
    pub fn contains(&self, function_id: &FunctionId) -> bool {
        self.summaries.contains_key(function_id)
            || self.builtin_summaries.contains_key(&function_id.0)
    }

    /// 获取或创建函数摘要
    pub fn get_or_create(
        &mut self,
        function_id: FunctionId,
        param_count: usize,
    ) -> &mut FunctionSummary {
        self.summaries
            .entry(function_id.clone())
            .or_insert_with(|| FunctionSummary::new(function_id, param_count))
    }

    /// 获取所有函数摘要
    pub fn all_summaries(&self) -> impl Iterator<Item = &FunctionSummary> {
        self.summaries.values()
    }

    /// 清空数据库（保留内置函数）
    pub fn clear(&mut self) {
        self.summaries.clear();
    }

    /// 数据库统计信息
    pub fn stats(&self) -> DatabaseStats {
        let total_functions = self.summaries.len();
        let mut no_escape_params = 0;
        let mut escape_params = 0;

        for summary in self.summaries.values() {
            for tag in &summary.parameter_tags {
                if tag.escapes() {
                    escape_params += 1;
                } else {
                    no_escape_params += 1;
                }
            }
        }

        DatabaseStats {
            total_functions,
            total_builtin_functions: self.builtin_summaries.len(),
            no_escape_params,
            escape_params,
        }
    }
}

/// 数据库统计信息
#[derive(Debug, Clone)]
pub struct DatabaseStats {
    /// 用户函数总数
    pub total_functions: usize,

    /// 内置函数总数
    pub total_builtin_functions: usize,

    /// 不逃逸参数数量
    pub no_escape_params: usize,

    /// 逃逸参数数量
    pub escape_params: usize,
}

impl DatabaseStats {
    /// 计算不逃逸参数的百分比
    pub fn no_escape_percentage(&self) -> f64 {
        let total = self.no_escape_params + self.escape_params;
        if total == 0 {
            0.0
        } else {
            (self.no_escape_params as f64 / total as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameter_tag_merge() {
        let no_escape = ParameterTag::NoEscape;
        let return_escape = ParameterTag::EscapeViaReturn;
        let heap_escape = ParameterTag::EscapeViaHeapStore;

        // NoEscape + ReturnEscape = ReturnEscape
        assert_eq!(
            no_escape.merge(&return_escape),
            ParameterTag::EscapeViaReturn
        );

        // ReturnEscape + HeapStore = HeapStore
        assert_eq!(
            return_escape.merge(&heap_escape),
            ParameterTag::EscapeViaHeapStore
        );
    }

    #[test]
    fn test_function_summary() {
        let func_id = FunctionId("test".to_string());
        let mut summary = FunctionSummary::new(func_id, 2);

        // 初始状态：所有参数不逃逸
        assert!(!summary.parameter_escapes(0));
        assert!(!summary.parameter_escapes(1));

        // 标记第0个参数逃逸
        summary.mark_parameter_escape(0, ParameterTag::EscapeViaReturn);
        assert!(summary.parameter_escapes(0));
        assert!(!summary.parameter_escapes(1));
    }

    #[test]
    fn test_function_summary_database() {
        let mut db = FunctionSummaryDatabase::new();

        // 内置函数应该存在
        assert!(db.contains(&FunctionId("print".to_string())));

        // 添加用户函数
        let func_id = FunctionId("user_func".to_string());
        let summary = FunctionSummary::new(func_id.clone(), 1);
        db.insert(summary);

        assert!(db.contains(&func_id));
        assert!(db.get(&func_id).is_some());
    }

    #[test]
    fn test_builtin_summaries() {
        let db = FunctionSummaryDatabase::new();

        // print 函数不保存参数
        let print_summary = db.get(&FunctionId("print".to_string())).unwrap();
        assert_eq!(print_summary.parameter_tags[0], ParameterTag::NoEscape);
        assert!(print_summary.modifies_global_state); // 有IO副作用
    }
}
