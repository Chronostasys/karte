//! 程序管理器
//!
//! 负责加载LIR程序，建立标签映射，管理程序代码

use super::MainFunctionInfo;
use karte_lir::{Instruction, LabelId, LirFunction, LirProgram};
use std::collections::HashMap;

/// 程序管理器
///
/// 管理程序代码、标签映射和函数信息
#[derive(Debug)]
pub struct ProgramManager {
    /// 所有指令的线性序列
    instructions: Vec<Instruction>,
    /// 标签到程序计数器的映射
    label_map: HashMap<LabelId, usize>,
    /// 函数名到函数信息的映射
    function_map: HashMap<String, FunctionInfo>,
    /// 主函数信息
    main_function: Option<MainFunctionInfo>,
}

/// 函数信息
#[derive(Debug, Clone)]
pub struct FunctionInfo {
    /// 函数名称
    pub name: String,
    /// 开始PC
    pub start_pc: usize,
    /// 结束PC（不包含）
    pub end_pc: usize,
    /// 入口标签
    pub entry_label: Option<LabelId>,
    /// 函数对象的克隆（用于获取参数等信息）
    pub function: LirFunction,
}

impl ProgramManager {
    /// 创建新的程序管理器
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            label_map: HashMap::new(),
            function_map: HashMap::new(),
            main_function: None,
        }
    }

    /// 加载LIR程序
    pub fn load_program(&mut self, program: &LirProgram) -> crate::Result<()> {
        // 清空之前的状态
        self.instructions.clear();
        self.label_map.clear();
        self.function_map.clear();
        self.main_function = None;

        // 构建指令序列和映射
        self.build_instruction_sequence(program)?;

        // 设置主函数信息
        self.setup_main_function(program)?;

        Ok(())
    }

    /// 构建指令序列和标签映射
    fn build_instruction_sequence(&mut self, program: &LirProgram) -> crate::Result<()> {
        let mut current_pc = 0;

        // 按照固定顺序处理函数（主函数优先）
        let mut functions_to_process = Vec::new();

        // 首先添加主函数
        if let Some(main_name) = &program.main_function {
            if let Some(main_func) = program.functions.get(main_name) {
                functions_to_process.push((main_name.clone(), main_func));
            }
        }

        // 然后添加其他函数
        for (name, function) in &program.functions {
            if Some(name) != program.main_function.as_ref() {
                functions_to_process.push((name.clone(), function));
            }
        }

        // 处理每个函数
        for (func_name, function) in functions_to_process {
            let start_pc = current_pc;

            // 添加函数的所有指令
            for instruction in &function.instructions {
                self.instructions.push(instruction.clone());
                current_pc += 1;
            }

            let end_pc = current_pc;

            // 建立函数内的标签映射
            let mut entry_label = None;
            for (index, instruction) in function.instructions.iter().enumerate() {
                if let Instruction::Label { id, .. } = instruction {
                    let pc = start_pc + index;
                    self.label_map.insert(*id, pc);

                    // 记录第一个标签作为入口
                    if entry_label.is_none() {
                        entry_label = Some(*id);
                    }
                }
            }

            // 保存函数信息
            let func_info = FunctionInfo {
                name: func_name.clone(),
                start_pc,
                end_pc,
                entry_label,
                function: function.clone(),
            };

            self.function_map.insert(func_name, func_info);
        }

        Ok(())
    }

    /// 设置主函数信息
    fn setup_main_function(&mut self, program: &LirProgram) -> crate::Result<()> {
        let main_name = program
            .main_function
            .as_ref()
            .ok_or("No main function specified")?;

        let func_info = self
            .function_map
            .get(main_name)
            .ok_or("Main function not found in function map")?;

        let entry_label = func_info
            .entry_label
            .ok_or("Main function has no entry label")?;

        let entry_pc = *self
            .label_map
            .get(&entry_label)
            .ok_or("Cannot find PC for main function entry label")?;

        self.main_function = Some(MainFunctionInfo {
            name: main_name.clone(),
            entry_pc,
            entry_label,
        });

        Ok(())
    }

    /// 获取主函数信息
    pub fn get_main_function_info(&self) -> crate::Result<&MainFunctionInfo> {
        self.main_function
            .as_ref()
            .ok_or_else(|| "Main function not loaded".into())
    }

    /// 获取指定PC处的指令
    pub fn get_instruction_at_pc(&self, pc: usize) -> crate::Result<&Instruction> {
        self.instructions
            .get(pc)
            .ok_or_else(|| format!("Invalid PC: {}", pc).into())
    }

    /// 获取标签对应的PC
    pub fn get_label_pc(&self, label: &LabelId) -> crate::Result<usize> {
        self.label_map
            .get(label)
            .copied()
            .ok_or_else(|| format!("Unknown label: {:?}", label).into())
    }

    /// 获取指令总数
    pub fn instruction_count(&self) -> usize {
        self.instructions.len()
    }

    /// 获取函数信息
    pub fn get_function_info(&self, name: &str) -> Option<&FunctionInfo> {
        self.function_map.get(name)
    }

    /// 获取所有函数信息
    pub fn get_all_functions(&self) -> &HashMap<String, FunctionInfo> {
        &self.function_map
    }

    /// 根据PC获取当前所在的函数
    pub fn get_function_at_pc(&self, pc: usize) -> Option<&FunctionInfo> {
        self.function_map
            .values()
            .find(|&func_info| pc >= func_info.start_pc && pc < func_info.end_pc)
    }

    /// 获取所有标签信息（调试用）
    pub fn get_all_labels(&self) -> &HashMap<LabelId, usize> {
        &self.label_map
    }

    /// 打印程序信息（调试用）
    pub fn print_program_info(&self) {
        println!("=== 程序信息 ===");
        println!("指令总数: {}", self.instructions.len());
        println!("标签数量: {}", self.label_map.len());
        println!("函数数量: {}", self.function_map.len());

        if let Some(main_info) = &self.main_function {
            println!("主函数: {} (PC: {})", main_info.name, main_info.entry_pc);
        }

        println!("函数列表:");
        for (name, info) in &self.function_map {
            println!("  {}: PC {}-{}", name, info.start_pc, info.end_pc);
        }

        println!("标签映射:");
        for (label, pc) in &self.label_map {
            println!("  {:?} -> PC {}", label, pc);
        }
    }
}

impl Default for ProgramManager {
    fn default() -> Self {
        Self::new()
    }
}
