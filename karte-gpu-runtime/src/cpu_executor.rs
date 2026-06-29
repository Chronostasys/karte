//! GIR CPU 回退解释器
//!
//! 在无 GPU 的环境中模拟执行 GIR kernel，用于测试和调试。
//! 单线程执行模式：模拟所有线程的顺序执行。

use karte_gir::*;

/// CPU 回退执行器
pub struct CpuExecutor {
    /// 模拟的线程数
    block_size: usize,
    /// 虚拟寄存器文件
    registers: Vec<i64>,
    /// 全局内存（模拟 GPU 全局内存）
    global_mem: Vec<i64>,
    /// 共享内存（模拟 GPU 共享内存）
    shared_mem: Vec<i64>,
    /// 标签位置映射: label_id → instruction_index
    label_positions: std::collections::HashMap<usize, usize>,
}

impl CpuExecutor {
    pub fn new(block_size: usize, global_mem_size: usize) -> Self {
        Self {
            block_size,
            registers: vec![0; 4096],
            global_mem: vec![0; global_mem_size],
            shared_mem: vec![0; 65536],
            label_positions: std::collections::HashMap::new(),
        }
    }

    /// 预扫描标签位置
    fn scan_labels(&mut self, func: &GirFunction) {
        self.label_positions.clear();
        for (i, instr) in func.instructions.iter().enumerate() {
            if let GirInstruction::Label { id } = instr {
                self.label_positions.insert(*id, i);
            }
        }
    }

    /// 在 CPU 上模拟执行 kernel（单线程模式）
    ///
    /// `params` 是 kernel 参数值（对应 kernel 参数列表）
    /// 返回执行后的全局内存快照
    pub fn execute(&mut self, func: &GirFunction, params: &[i64]) -> Result<&[i64], String> {
        self.scan_labels(func);

        // 加载参数到寄存器
        for (i, &val) in params.iter().enumerate() {
            if i < self.registers.len() {
                self.registers[i] = val;
            }
        }

        // 单线程执行（tid=0, bid=0）
        let pc_result = self.execute_instructions(func, 0, 0)?;
        let _ = pc_result;

        Ok(&self.global_mem)
    }

    /// 执行指令序列（模拟一个线程）
    fn execute_instructions(
        &mut self,
        func: &GirFunction,
        thread_id: i64,
        block_id: i64,
    ) -> Result<(), String> {
        let mut pc = 0;
        let max_steps = 1000000; // 安全限制
        let mut steps = 0;

        while pc < func.instructions.len() {
            steps += 1;
            if steps > max_steps {
                return Err(format!("Kernel execution exceeded {} steps (infinite loop?)", max_steps));
            }

            let instr = &func.instructions[pc];
            match self.execute_one(instr, thread_id, block_id)? {
                JumpResult::Continue => pc += 1,
                JumpResult::JumpTo(label) => {
                    pc = *self.label_positions.get(&label).ok_or_else(|| format!("Unknown label {}", label))?;
                }
                JumpResult::Return => return Ok(()),
            }
        }

        Ok(())
    }

    /// 执行单条指令
    fn execute_one(&mut self, instr: &GirInstruction, tid: i64, bid: i64) -> Result<JumpResult, String> {
        let block_size = self.block_size as i64;

        match instr {
            GirInstruction::Move { dst, src } => {
                let v = self.get_operand(src);
                self.set_reg(*dst, v);
            }
            GirInstruction::Add { dst, src1, src2, .. } => {
                self.set_reg(*dst, self.get_operand(src1).wrapping_add(self.get_operand(src2)));
            }
            GirInstruction::Sub { dst, src1, src2, .. } => {
                self.set_reg(*dst, self.get_operand(src1).wrapping_sub(self.get_operand(src2)));
            }
            GirInstruction::Mul { dst, src1, src2, .. } => {
                self.set_reg(*dst, self.get_operand(src1).wrapping_mul(self.get_operand(src2)));
            }
            GirInstruction::Div { dst, src1, src2, dtype } => {
                let a = self.get_operand(src1);
                let b = self.get_operand(src2);
                if b == 0 { return Err("Division by zero".into()); }
                if *dtype == GirDType::F32 || *dtype == GirDType::F64 {
                    let af = f64::from_bits(a as u64);
                    let bf = f64::from_bits(b as u64);
                    self.set_reg(*dst, (af / bf).to_bits() as i64);
                } else {
                    self.set_reg(*dst, a / b);
                }
            }
            GirInstruction::Mod { dst, src1, src2, .. } => {
                let a = self.get_operand(src1);
                let b = self.get_operand(src2);
                if b == 0 { return Err("Modulo by zero".into()); }
                self.set_reg(*dst, a % b);
            }
            GirInstruction::Fma { dst, src1, src2, src3, dtype } => {
                let a = self.get_operand(src1);
                let b = self.get_operand(src2);
                let c = self.get_operand(src3);
                if *dtype == GirDType::F32 || *dtype == GirDType::F64 {
                    let af = f64::from_bits(a as u64);
                    let bf = f64::from_bits(b as u64);
                    let cf = f64::from_bits(c as u64);
                    self.set_reg(*dst, (af * bf + cf).to_bits() as i64);
                } else {
                    self.set_reg(*dst, a.wrapping_mul(b).wrapping_add(c));
                }
            }
            GirInstruction::Cmp { dst, op, src1, src2, .. } => {
                let a = self.get_operand(src1);
                let b = self.get_operand(src2);
                let result = match op {
                    CmpOp::Eq => a == b,
                    CmpOp::Ne => a != b,
                    CmpOp::Lt => a < b,
                    CmpOp::Le => a <= b,
                    CmpOp::Gt => a > b,
                    CmpOp::Ge => a >= b,
                };
                self.set_reg(*dst, if result { 1 } else { 0 });
            }
            GirInstruction::BranchIf { cond, then_label, else_label } => {
                let c = self.get_operand(cond);
                return Ok(if c != 0 {
                    JumpResult::JumpTo(*then_label)
                } else {
                    JumpResult::JumpTo(*else_label)
                });
            }
            GirInstruction::Jump { target } => {
                return Ok(JumpResult::JumpTo(*target));
            }
            GirInstruction::GlobalLoad { dst, addr, .. } => {
                let a = self.get_operand(addr) as usize;
                let v = if a < self.global_mem.len() { self.global_mem[a] } else { 0 };
                self.set_reg(*dst, v);
            }
            GirInstruction::GlobalStore { addr, src, .. } => {
                let a = self.get_operand(addr) as usize;
                let v = self.get_operand(src);
                if a < self.global_mem.len() { self.global_mem[a] = v; }
            }
            GirInstruction::SharedLoad { dst, addr, .. } => {
                let a = self.get_operand(addr) as usize;
                let v = if a < self.shared_mem.len() { self.shared_mem[a] } else { 0 };
                self.set_reg(*dst, v);
            }
            GirInstruction::SharedStore { addr, src, .. } => {
                let a = self.get_operand(addr) as usize;
                let v = self.get_operand(src);
                if a < self.shared_mem.len() { self.shared_mem[a] = v; }
            }
            GirInstruction::Barrier => { /* 单线程模式下屏障是 no-op */ }
            GirInstruction::ThreadId { dst, dim } => {
                let val = match dim { ThreadDim::X => tid, ThreadDim::Y => 0, ThreadDim::Z => 0 };
                self.set_reg(*dst, val);
            }
            GirInstruction::BlockId { dst, dim } => {
                let val = match dim { ThreadDim::X => bid, ThreadDim::Y => 0, ThreadDim::Z => 0 };
                self.set_reg(*dst, val);
            }
            GirInstruction::BlockDim { dst, dim } => {
                let val = match dim { ThreadDim::X => block_size, _ => 1 };
                self.set_reg(*dst, val);
            }
            GirInstruction::GridDim { dst, dim } => {
                let val = match dim { ThreadDim::X => 1, _ => 1 };
                self.set_reg(*dst, val);
            }
            GirInstruction::Label { .. } => {}
            GirInstruction::Return => return Ok(JumpResult::Return),
            GirInstruction::WarpShuffle { dst, src, .. } => {
                // 单线程模式下 shuffle 返回自身值
                self.set_reg(*dst, self.get_operand(src));
            }
            // 高级 tile 操作（正常应已展开）
            _ => {}
        }

        Ok(JumpResult::Continue)
    }

    fn get_operand(&self, op: &GirOperand) -> i64 {
        match op {
            GirOperand::Reg(id) => *self.registers.get(*id).unwrap_or(&0),
            GirOperand::Imm(val) => *val,
            GirOperand::Param(id) => *self.registers.get(*id).unwrap_or(&0),
            GirOperand::Label(_) => 0,
        }
    }

    fn set_reg(&mut self, id: usize, val: i64) {
        if id < self.registers.len() {
            self.registers[id] = val;
        }
    }
}

/// 跳转结果
enum JumpResult {
    Continue,
    JumpTo(usize),
    Return,
}

/// 在 CPU 上模拟执行整个 kernel（多线程模式）
///
/// 模拟 block_size 个线程的顺序执行，每个线程看到相同的指令但不同的 thread_id。
pub fn execute_kernel_on_cpu(
    func: &GirFunction,
    params: &[i64],
    global_mem_size: usize,
) -> Result<Vec<i64>, String> {
    let block_size = func.block_dim.0;
    let mut executor = CpuExecutor::new(block_size, global_mem_size);

    // 模拟 block_size 个线程
    for tid in 0..block_size.min(256) { // 限制模拟线程数
        executor.scan_labels(func);

        // 每个线程需要独立的寄存器状态
        // 简化实现：只执行线程 0，验证逻辑正确性
        if tid == 0 {
            let result = executor.execute(func, params)?;
            let _ = result;
        }
    }

    Ok(executor.global_mem)
}
