//! 寄存器生命周期分析
//! 
//! 分析虚拟寄存器在程序中的使用范围，为寄存器分配提供基础数据。
//! 包括控制流分析、循环检测和调用点分析。

use karte_lir::{Instruction, LirFunction, RegisterId, Operand, LabelId};
use std::collections::{HashMap, HashSet, BTreeSet};

/// 寄存器生命周期信息
#[derive(Debug, Clone, PartialEq)]
pub struct RegisterLifetime {
    /// 寄存器ID
    pub register: RegisterId,
    /// 首次使用位置
    pub start: usize,
    /// 最后使用位置
    pub end: usize,
    /// 权重（使用频率，用于溢出决策）
    pub weight: f64,
    /// 是否在循环中使用
    pub in_loop: bool,
    /// 是否是函数参数
    pub is_parameter: bool,
    /// 是否是返回值
    pub is_return_value: bool,
}

impl RegisterLifetime {
    /// 创建新的生命周期
    pub fn new(register: RegisterId, start: usize, end: usize) -> Self {
        Self {
            register,
            start,
            end,
            weight: 1.0,
            in_loop: false,
            is_parameter: false,
            is_return_value: false,
        }
    }

    /// 检查是否与另一个生命周期重叠
    pub fn overlaps_with(&self, other: &RegisterLifetime) -> bool {
        !(self.end < other.start || other.end < self.start)
    }

    /// 获取生命周期长度
    pub fn length(&self) -> usize {
        self.end.saturating_sub(self.start)
    }
}

/// 控制流图节点
#[derive(Debug, Clone)]
pub struct ControlFlowNode {
    /// 节点ID
    pub id: usize,
    /// 指令索引
    pub instruction_index: usize,
    /// 前驱节点
    pub predecessors: Vec<usize>,
    /// 后继节点
    pub successors: Vec<usize>,
    /// 是否是循环头
    pub is_loop_header: bool,
}

/// 函数调用上下文分析结果
#[derive(Debug, Clone)]
pub struct CallSiteContext {
    /// 调用点位置
    pub call_site: usize,
    /// 参数寄存器
    pub arguments: Vec<RegisterId>,
    /// 返回值寄存器
    pub return_register: Option<RegisterId>,
    /// 调用前活跃的寄存器
    pub live_before: HashSet<RegisterId>,
    /// 调用后活跃的寄存器
    pub live_after: HashSet<RegisterId>,
}

/// 生命周期分析器
#[derive(Debug)]
pub struct LifetimeAnalyzer {
    /// 控制流图
    control_flow: Vec<ControlFlowNode>,
    /// 标签到节点的映射
    label_to_node: HashMap<LabelId, usize>,
    /// 循环信息
    loops: Vec<LoopInfo>,
}

/// 循环信息
#[derive(Debug, Clone)]
pub struct LoopInfo {
    /// 循环头节点
    pub header: usize,
    /// 循环体节点集合
    pub body: HashSet<usize>,
    /// 嵌套深度
    pub depth: usize,
}

impl LifetimeAnalyzer {
    /// 创建新的生命周期分析器
    pub fn new() -> Self {
        Self {
            control_flow: Vec::new(),
            label_to_node: HashMap::new(),
            loops: Vec::new(),
        }
    }

    /// 分析函数的寄存器生命周期
    pub fn analyze_function(&mut self, function: &LirFunction) -> Result<Vec<RegisterLifetime>, String> {
        // 1. 构建控制流图
        self.build_control_flow_graph(function)?;
        
        // 2. 检测循环
        self.detect_loops();
        
        // 3. 分析寄存器使用
        let mut lifetimes = self.analyze_register_usage(function)?;
        
        // 4. 计算权重
        self.calculate_weights(&mut lifetimes);
        
        // 5. 按开始位置排序
        lifetimes.sort_by_key(|lt| lt.start);
        
        Ok(lifetimes)
    }

    /// 构建控制流图
    fn build_control_flow_graph(&mut self, function: &LirFunction) -> Result<(), String> {
        self.control_flow.clear();
        self.label_to_node.clear();
        
        // 为每条指令创建一个节点
        for (index, instruction) in function.instructions.iter().enumerate() {
            let mut node = ControlFlowNode {
                id: index,
                instruction_index: index,
                predecessors: Vec::new(),
                successors: Vec::new(),
                is_loop_header: false,
            };
            
            // 记录标签位置
            if let Instruction::Label { id, .. } = instruction {
                self.label_to_node.insert(*id, index);
            }
            
            self.control_flow.push(node);
        }
        
        // 建立边连接
        for (index, instruction) in function.instructions.iter().enumerate() {
            match instruction {
                // 无条件跳转
                Instruction::Jump { target, .. } => {
                    if let Some(&target_node) = self.label_to_node.get(target) {
                        self.add_edge(index, target_node);
                    }
                }
                
                // 条件跳转
                Instruction::JumpEqual { target, .. } |
                Instruction::JumpNotEqual { target, .. } |
                Instruction::JumpGreater { target, .. } |
                Instruction::JumpGreaterEqual { target, .. } |
                Instruction::JumpLess { target, .. } |
                Instruction::JumpLessEqual { target, .. } => {
                    // 跳转目标
                    if let Some(&target_node) = self.label_to_node.get(target) {
                        self.add_edge(index, target_node);
                    }
                    // 顺序执行
                    if index + 1 < self.control_flow.len() {
                        self.add_edge(index, index + 1);
                    }
                }
                
                // 返回指令没有后继
                Instruction::Return { .. } => {}
                
                // 其他指令正常顺序执行
                _ => {
                    if index + 1 < self.control_flow.len() {
                        self.add_edge(index, index + 1);
                    }
                }
            }
        }
        
        Ok(())
    }

    /// 添加控制流边
    fn add_edge(&mut self, from: usize, to: usize) {
        if from < self.control_flow.len() && to < self.control_flow.len() {
            self.control_flow[from].successors.push(to);
            self.control_flow[to].predecessors.push(from);
        }
    }

    /// 检测循环
    fn detect_loops(&mut self) {
        self.loops.clear();
        
        // 使用深度优先搜索检测后向边（back edges）
        let mut visited = vec![false; self.control_flow.len()];
        let mut in_stack = vec![false; self.control_flow.len()];
        
        for i in 0..self.control_flow.len() {
            if !visited[i] {
                self.dfs_detect_loops(i, &mut visited, &mut in_stack);
            }
        }
    }

    /// 深度优先搜索检测循环
    fn dfs_detect_loops(&mut self, node: usize, visited: &mut Vec<bool>, in_stack: &mut Vec<bool>) {
        visited[node] = true;
        in_stack[node] = true;
        
        for &successor in &self.control_flow[node].successors.clone() {
            if !visited[successor] {
                self.dfs_detect_loops(successor, visited, in_stack);
            } else if in_stack[successor] {
                // 发现后向边，这是一个循环
                self.process_loop(successor, node);
            }
        }
        
        in_stack[node] = false;
    }

    /// 处理发现的循环
    fn process_loop(&mut self, header: usize, back_edge_source: usize) {
        let mut loop_body = HashSet::new();
        let mut stack = vec![back_edge_source];
        loop_body.insert(header);
        
        // 收集循环体中的所有节点
        while let Some(node) = stack.pop() {
            if loop_body.insert(node) {
                // 添加所有前驱节点
                for &pred in &self.control_flow[node].predecessors {
                    if !loop_body.contains(&pred) {
                        stack.push(pred);
                    }
                }
            }
        }
        
        // 标记循环头
        if header < self.control_flow.len() {
            self.control_flow[header].is_loop_header = true;
        }
        
        // 创建循环信息
        let loop_info = LoopInfo {
            header,
            body: loop_body,
            depth: 1, // 简化处理，嵌套深度为1
        };
        
        self.loops.push(loop_info);
    }

    /// 分析寄存器使用
    fn analyze_register_usage(&self, function: &LirFunction) -> Result<Vec<RegisterLifetime>, String> {
        let mut register_uses: HashMap<RegisterId, Vec<usize>> = HashMap::new();
        
        // 收集每个寄存器的所有使用位置
        for (pos, instruction) in function.instructions.iter().enumerate() {
            let used_registers = self.extract_registers_from_instruction(instruction);
            
            for reg in used_registers {
                register_uses.entry(reg).or_insert_with(Vec::new).push(pos);
            }
        }
        
        // 为每个寄存器创建生命周期
        let mut lifetimes = Vec::new();
        for (register, positions) in register_uses {
            if let (Some(&first), Some(&last)) = (positions.first(), positions.last()) {
                let mut lifetime = RegisterLifetime::new(register, first, last);
                
                // 检查是否在循环中使用
                lifetime.in_loop = self.is_register_in_loop(register, &positions);
                
                // 计算初始权重
                lifetime.weight = positions.len() as f64;
                
                lifetimes.push(lifetime);
            }
        }
        
        Ok(lifetimes)
    }

    /// 检查寄存器是否在循环中使用
    fn is_register_in_loop(&self, _register: RegisterId, positions: &[usize]) -> bool {
        for &pos in positions {
            for loop_info in &self.loops {
                if loop_info.body.contains(&pos) {
                    return true;
                }
            }
        }
        false
    }

    /// 从指令中提取寄存器
    fn extract_registers_from_instruction(&self, instruction: &Instruction) -> Vec<RegisterId> {
        let mut registers = Vec::new();

        match instruction {
            Instruction::Move { dst, src, .. } => {
                registers.push(*dst);
                if let Operand::Register { id } = src {
                    registers.push(*id);
                }
            }
            
            Instruction::Add { dst, src1, src2, .. } |
            Instruction::Sub { dst, src1, src2, .. } |
            Instruction::Mul { dst, src1, src2, .. } |
            Instruction::Div { dst, src1, src2, .. } => {
                registers.push(*dst);
                if let Operand::Register { id } = src1 {
                    registers.push(*id);
                }
                if let Operand::Register { id } = src2 {
                    registers.push(*id);
                }
            }
            
            Instruction::Compare { src1, src2, .. } => {
                if let Operand::Register { id } = src1 {
                    registers.push(*id);
                }
                if let Operand::Register { id } = src2 {
                    registers.push(*id);
                }
            }
            
            Instruction::Call { args, result, .. } => {
                registers.extend_from_slice(args);
                if let Some(reg) = result {
                    registers.push(*reg);
                }
            }
            
            Instruction::Return { value, .. } => {
                if let Some(reg) = value {
                    registers.push(*reg);
                }
            }
            
            _ => {} // 其他指令暂时不处理
        }

        registers
    }

    /// 计算寄存器权重
    fn calculate_weights(&self, lifetimes: &mut [RegisterLifetime]) {
        for lifetime in lifetimes.iter_mut() {
            // 基础权重 = 使用次数
            let mut weight = lifetime.weight;
            
            // 循环中的寄存器权重增加
            if lifetime.in_loop {
                weight *= 10.0;
            }
            
            // 生命周期越长，权重越高（更难溢出）
            weight += lifetime.length() as f64 * 0.1;
            
            lifetime.weight = weight;
        }
    }

    /// 分析函数调用点
    pub fn analyze_call_site(&self, 
                           function: &LirFunction, 
                           call_site: usize, 
                           args: &[RegisterId],
                           return_reg: Option<RegisterId>) -> Result<CallSiteContext, String> {
        // 简化实现，实际应该做数据流分析
        let live_before = args.iter().copied().collect();
        let live_after = return_reg.into_iter().collect();
        
        Ok(CallSiteContext {
            call_site,
            arguments: args.to_vec(),
            return_register: return_reg,
            live_before,
            live_after,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use karte_lir::{Instruction, Operand};
    use karte_diagnostics::Span;

    #[test]
    fn test_lifetime_analysis() {
        let mut analyzer = LifetimeAnalyzer::new();
        let mut function = LirFunction::new("test".to_string());
        
        // 添加一些测试指令
        function.add_instruction(Instruction::Move {
            dst: RegisterId(1),
            src: Operand::Immediate { value: 42 },
            span: Span::dummy(),
        });
        
        function.add_instruction(Instruction::Add {
            dst: RegisterId(2),
            src1: Operand::Register { id: RegisterId(1) },
            src2: Operand::Immediate { value: 1 },
            span: Span::dummy(),
        });
        
        let lifetimes = analyzer.analyze_function(&function).unwrap();
        
        assert_eq!(lifetimes.len(), 2);
        assert_eq!(lifetimes[0].register, RegisterId(1));
        assert_eq!(lifetimes[1].register, RegisterId(2));
    }

    #[test]
    fn test_register_overlap() {
        let lt1 = RegisterLifetime::new(RegisterId(1), 0, 5);
        let lt2 = RegisterLifetime::new(RegisterId(2), 3, 8);
        let lt3 = RegisterLifetime::new(RegisterId(3), 6, 10);
        
        assert!(lt1.overlaps_with(&lt2)); // 0-5 和 3-8 重叠
        assert!(!lt1.overlaps_with(&lt3)); // 0-5 和 6-10 不重叠
        assert!(lt2.overlaps_with(&lt3)); // 3-8 和 6-10 重叠
    }
} 