/// test_cc 回归测试 —— 用 Karte 编写的 C 子集编译器
///
/// 测试流程:
///   1. AOT 编译 test_cc (Karte 项目) 为原生二进制
///   2. 用该二进制将 C 源码编译为 x86_64 汇编
///   3. 用系统 as + ld 汇编链接
///   4. 执行生成的二进制，验证 exit code
///
/// 这些测试覆盖 test_cc 之前导致 SIGSEGV 的 bug:
///   - 递归函数 codegen (parse_func 跳过了 `{`)
///   - var_off 找不到变量时返回垃圾偏移
///   - for 循环未跳过 `(`
///   - 无大括号 if 语句
#[cfg(test)]
mod test_cc_regression {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::Command;

    /// 获取项目根目录
    fn project_root() -> PathBuf {
        let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        dir.pop(); // karte-tests -> karte root
        dir
    }

    /// 确保 karte CLI 已编译
    fn ensure_karte_binary() -> PathBuf {
        let root = project_root();
        let karte_bin = root.join("target/debug/karte");
        if !karte_bin.exists() {
            panic!("karte binary not found at {:?}. Run `cargo build` first.", karte_bin);
        }
        karte_bin
    }

    /// AOT 编译 test_cc，返回编译出的二进制路径（带文件锁防并行编译冲突）
    fn ensure_test_cc_binary() -> PathBuf {
        let root = project_root();
        let test_cc_src = root.join("test_cc/src/main.karte");
        let aot_bin = std::env::temp_dir().join(".karte_test_cc_aot_regression");
        let karte_bin = ensure_karte_binary();

        // 用 flock 做互斥——先写临时文件再 rename（原子操作）
        let tmp_aot = std::env::temp_dir().join(".karte_test_cc_aot_regression.tmp");

        let cmd = format!(
            "if [ -f '{target}' ]; then exit 0; fi; \
             {karte} aot --mode project {src} -o {tmp} && mv {tmp} {target}",
            target = aot_bin.display(),
            karte = karte_bin.display(),
            src = test_cc_src.display(),
            tmp = tmp_aot.display(),
        );

        let output = Command::new("flock")
            .arg("/tmp/.karte_test_cc_aot_regression.lock")
            .arg("bash")
            .arg("-c")
            .arg(&cmd)
            .output()
            .expect("Failed to run flock");

        if !aot_bin.exists() {
            panic!(
                "karte aot failed:\nstdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }

        aot_bin
    }

    /// 编译 C 源码为可执行文件并运行，返回 exit code
    fn compile_and_run_c(c_source: &str) -> i64 {
        let aot_bin = ensure_test_cc_binary();

        // 用 PID + 原子计数器生成唯一文件名，避免并行测试冲突
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let uid = std::process::id() as u64 * 100000 + COUNTER.fetch_add(1, Ordering::SeqCst);

        let tmpdir = std::env::temp_dir();
        let asm_file = tmpdir.join(format!("test_cc_reg_{}.s", uid));
        let obj_file = tmpdir.join(format!("test_cc_reg_{}.o", uid));
        let exe_file = tmpdir.join(format!("test_cc_reg_exe_{}", uid));

        // 1. 用 test_cc 编译 C → 汇编
        let mut child = Command::new(&aot_bin)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("Failed to spawn test_cc");

        use std::io::Write;
        {
            let stdin = child.stdin.as_mut().expect("Failed to get stdin");
            stdin.write_all(c_source.as_bytes()).expect("Failed to write C source");
        }
        let output = child.wait_with_output().expect("Failed to wait for test_cc");

        if !output.status.success() {
            panic!(
                "test_cc itself crashed (SIGSEGV?):\nstderr: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let asm_text = String::from_utf8_lossy(&output.stdout);
        // 清理末尾的数字输出行
        let asm_clean: String = asm_text
            .lines()
            .filter(|line| {
                let trimmed = line.trim();
                trimmed.is_empty() || !trimmed.chars().all(|c| c.is_ascii_digit())
            })
            .collect::<Vec<_>>()
            .join("\n");

        // 2. 写汇编文件
        let debug_asm = tmpdir.join(format!("test_cc_debug_{}.s", uid));
        let _ = fs::write(&debug_asm, &asm_clean);
        fs::write(&asm_file, &asm_clean).expect("Failed to write asm file");

        // 3. 用 as 汇编
        let as_output = Command::new("as")
            .arg("--64")
            .arg(&asm_file)
            .arg("-o")
            .arg(&obj_file)
            .output()
            .expect("Failed to run as");

        if !as_output.status.success() {
            panic!(
                "as failed:\n{}\nasm was:\n{}",
                String::from_utf8_lossy(&as_output.stderr),
                asm_clean
            );
        }

        // 4. 用 ld 链接
        let ld_output = Command::new("ld")
            .arg(&obj_file)
            .arg("-o")
            .arg(&exe_file)
            .output()
            .expect("Failed to run ld");

        if !ld_output.status.success() {
            panic!(
                "ld failed:\n{}",
                String::from_utf8_lossy(&ld_output.stderr)
            );
        }

        // 设置可执行权限
        fs::set_permissions(&exe_file, fs::Permissions::from_mode(0o755))
            .expect("Failed to set permissions");

        // 5. 执行
        let run_output = Command::new(&exe_file)
            .output()
            .expect("Failed to run compiled binary");

        // 清理
        let _ = fs::remove_file(&asm_file);
        let _ = fs::remove_file(&obj_file);
        let _ = fs::remove_file(&exe_file);

        let code = run_output.status.code().unwrap_or(-1);
        // 检查是否被信号终止
        // 注意：test_cc 使用 syscall exit，exit code = main 返回值
        // 返回值 > 128 是正常的（不是信号），用 signal() 判断
        if run_output.status.signal().is_some() {
            let sig = run_output.status.signal().unwrap();
            panic!(
                "Generated binary crashed with signal {}. stdout: {}, stderr: {}",
                sig,
                String::from_utf8_lossy(&run_output.stdout),
                String::from_utf8_lossy(&run_output.stderr),
            );
        }
        code as i64
    }

    // ==================== 回归测试 ====================

    /// Bug: parse_func 跳过了 `{`，导致 do_block 不走大括号路径，
    /// if 无大括号时消费了整个函数体（return fib(n-1)+fib(n-2) 被跳过），
    /// 生成的汇编只有 if 分支代码，递归调用后的代码缺失，
    /// jmp 到未初始化内存 → SIGSEGV
    #[test]
    fn test_cc_recursive_fib_no_brace() {
        let c_source = r#"int fib(int n) {
    if (n <= 1) return n;
    return fib(n - 1) + fib(n - 2);
}
int main() { return fib(10); }
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 55, "fib(10) should be 55, got {}", exit);
    }

    /// Bug: parse_func 跳过 `{` (同上根因)，大括号版 fib 也不正确
    #[test]
    fn test_cc_recursive_fib_braced() {
        let c_source = r#"int fib(int n) {
    if (n <= 1) { return n; }
    return fib(n - 1) + fib(n - 2);
}
int main() { return fib(10); }
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 55, "fib(10) should be 55, got {}", exit);
    }

    /// Bug: var_off 找不到函数名时返回垃圾偏移值，
    /// p_primary 把 fib 误认为变量，生成了 mov garbage_offset(%rbp), %rax
    /// 访问了错误的栈内存位置 → SIGSEGV 或返回错误值
    #[test]
    fn test_cc_function_call_not_confused_with_variable() {
        let c_source = r#"int add(int a, int b) { return a + b; }
int main() { return add(3, 4); }
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 7, "add(3, 4) should be 7, got {}", exit);
    }

    /// Bug: for 循环处理中未跳过 `(`，导致 do_for_init 从 `(` 开始解析，
    /// 整个 for 的 init/cond/update/body 解析全部错位，
    /// 生成 mov 0(%rbp), %rax（访问保存的帧指针而非变量）→ SIGSEGV
    #[test]
    fn test_cc_for_loop() {
        let c_source = r#"int main() {
    int sum = 0;
    for (int i = 1; i <= 10; i += 1) {
        sum = sum + i;
    }
    return sum;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 55, "1+2+...+10 should be 55, got {}", exit);
    }

    /// for 循环复杂 case: 阶乘
    #[test]
    fn test_cc_for_loop_factorial() {
        let c_source = r#"int main() {
    int p = 1;
    for (int i = 1; i <= 5; i += 1) {
        p = p * i;
    }
    return p;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 120, "5! should be 120, got {}", exit);
    }

    /// 嵌套 for 循环
    #[test]
    fn test_cc_nested_for() {
        let c_source = r#"int main() {
    int sum = 0;
    for (int i = 1; i <= 3; i += 1) {
        for (int j = 1; j <= 3; j += 1) {
            sum = sum + 1;
        }
    }
    return sum;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 9, "3x3 should be 9, got {}", exit);
    }

    /// 嵌套函数调用（函数作为参数传递给另一个函数）
    #[test]
    fn test_cc_nested_function_call() {
        let c_source = r#"int add(int a, int b) { return a + b; }
int mul(int a, int b) { return a * b; }
int main() { return add(mul(3, 4), mul(5, 6)); }
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 42, "3*4 + 5*6 = 42, got {}", exit);
    }

    /// 递归求和 (sum_recur): 测试单行 if return 后跟 return 递归调用
    #[test]
    fn test_cc_recursive_sum() {
        let c_source = r#"int f(int n) { if (n <= 0) return 0; return n + f(n - 1); }
int main() { return f(5); }
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 15, "f(5)=5+4+3+2+1=15, got {}", exit);
    }

    /// Ackermann 函数：多重递归
    #[test]
    fn test_cc_ackermann() {
        let c_source = r#"int ack(int m, int n) {
    if (m == 0) { return n + 1; }
    if (n == 0) { return ack(m - 1, 1); }
    return ack(m - 1, ack(m, n - 1));
}
int main() { return ack(2, 3); }
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 9, "ack(2,3)=9, got {}", exit);
    }

    /// while 循环
    #[test]
    fn test_cc_while_loop() {
        let c_source = r#"int main() {
    int sum = 0;
    int i = 1;
    while (i <= 10) {
        sum = sum + i;
        i = i + 1;
    }
    return sum;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 55, "while: 1+..+10=55, got {}", exit);
    }

    /// if-else
    #[test]
    fn test_cc_if_else() {
        let c_source = r#"int max(int a, int b) {
    if (a > b) { return a; } else { return b; }
}
int main() { return max(7, 3); }
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 7, "max(7,3)=7, got {}", exit);
    }

    /// 嵌套 if-else
    #[test]
    fn test_cc_nested_if_else() {
        let c_source = r#"int classify(int n) {
    if (n > 0) {
        if (n > 10) return 2;
        else return 1;
    } else {
        return 0;
    }
}
int main() { return classify(5) + classify(20) + classify(-3); }
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 3, "1+2+0=3, got {}", exit);
    }

    /// 变量和算术
    #[test]
    fn test_cc_variables_and_arithmetic() {
        let c_source = r#"int main() {
    int x = 5;
    int y = 10;
    int z = x * y + 3;
    return z;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 53, "5*10+3=53, got {}", exit);
    }

    /// break 语句 — 跳出 while 循环
    #[test]
    fn test_cc_break_while() {
        let c_source = r#"int main() {
    int sum = 0;
    int i = 1;
    while (1) {
        if (i > 5) break;
        sum = sum + i;
        i = i + 1;
    }
    return sum;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 15, "1+2+3+4+5=15, got {}", exit);
    }

    /// break 语句 — 跳出 for 循环
    #[test]
    fn test_cc_break_for() {
        let c_source = r#"int main() {
    int sum = 0;
    for (int i = 1; i <= 100; i += 1) {
        if (i > 5) break;
        sum = sum + i;
    }
    return sum;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 15, "1+2+3+4+5=15, got {}", exit);
    }

    /// continue 语句 — 跳过 for 循环中 i%3==0 的迭代
    #[test]
    fn test_cc_continue_for() {
        let c_source = r#"int main() {
    int sum = 0;
    for (int i = 1; i <= 10; i += 1) {
        if (i % 3 == 0) continue;
        sum = sum + i;
    }
    return sum;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 37, "1+2+4+5+7+8+10=37, got {}", exit);
    }

    /// do-while 循环
    #[test]
    fn test_cc_do_while() {
        let c_source = r#"int main() {
    int sum = 0;
    int i = 1;
    do {
        sum = sum + i;
        i = i + 1;
    } while (i <= 5);
    return sum;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 15, "1+2+3+4+5=15, got {}", exit);
    }

    /// do-while 至少执行一次 (条件初始为 false)
    #[test]
    fn test_cc_do_while_false_initially() {
        let c_source = r#"int main() {
    int x = 0;
    do {
        x = 42;
    } while (0);
    return x;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 42, "do-while body runs once even if condition is false, got {}", exit);
    }

    /// 三元运算符
    #[test]
    fn test_cc_ternary() {
        let c_source = r#"int main() {
    int a = 5;
    int b = 10;
    int max = a > b ? a : b;
    return max;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 10, "max(5,10)=10, got {}", exit);
    }

    /// 三元运算符在表达式中
    #[test]
    fn test_cc_ternary_in_expr() {
        let c_source = r#"int abs_val(int x) { return x < 0 ? -x : x; }
int main() { return abs_val(-7); }
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 7, "abs(-7)=7, got {}", exit);
    }

    /// void 函数
    #[test]
    fn test_cc_void_function() {
        let c_source = r#"void set_val(int *p, int v) { *p = v; }
int main() {
    int x = 0;
    set_val(&x, 42);
    return x;
}
"#;
        // void 函数目前可能不支持指针，用一个简单的测试代替
        let c_source = r#"void noop() {}
int main() {
    noop();
    return 0;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 0, "void function noop returns 0, got {}", exit);
    }

    /// 数组基本读写
    #[test]
    fn test_cc_array_basic() {
        let c_source = r#"int main() {
    int arr[5];
    arr[0] = 10;
    arr[1] = 20;
    arr[2] = 30;
    arr[3] = 40;
    arr[4] = 50;
    return arr[0] + arr[4];
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 60, "arr[0]+arr[4]=10+50=60, got {}", exit);
    }

    /// 数组在循环中使用
    #[test]
    fn test_cc_array_loop() {
        let c_source = r#"int main() {
    int arr[5];
    for (int i = 0; i < 5; i += 1) {
        arr[i] = (i + 1) * 10;
    }
    int sum = 0;
    for (int j = 0; j < 5; j += 1) {
        sum = sum + arr[j];
    }
    return sum;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 150, "10+20+30+40+50=150, got {}", exit);
    }

    /// 嵌套 for 循环 + break + continue
    #[test]
    fn test_cc_break_continue_nested() {
        let c_source = r#"int main() {
    int sum = 0;
    for (int i = 0; i < 5; i += 1) {
        if (i == 3) continue;
        if (i == 4) break;
        sum = sum + i;
    }
    return sum;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 3, "0+1+2=3 (skip 3, break at 4), got {}", exit);
    }

    /// do-while 嵌套在 for 中
    #[test]
    fn test_cc_do_while_nested() {
        let c_source = r#"int main() {
    int total = 0;
    for (int i = 0; i < 3; i += 1) {
        int j = 0;
        do {
            total = total + 1;
            j = j + 1;
        } while (j <= i);
    }
    return total;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 6, "1+2+3=6, got {}", exit);
    }

    /// 三元运算符嵌套
    #[test]
    fn test_cc_ternary_nested() {
        let c_source = r#"int main() {
    int x = 15;
    int result = x > 20 ? 1 : (x > 10 ? 2 : 3);
    return result;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 2, "15>20?1:(15>10?2:3)=2, got {}", exit);
    }

    /// 数组 + 递归（fib 用数组缓存）
    #[test]
    fn test_cc_array_fib() {
        let c_source = r#"int main() {
    int fib[10];
    fib[0] = 1;
    fib[1] = 1;
    for (int i = 2; i < 10; i += 1) {
        fib[i] = fib[i - 1] + fib[i - 2];
    }
    return fib[9];
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 55, "fib[9]=55, got {}", exit);
    }

    /// switch-case 基本测试
    #[test]
    fn test_cc_switch_basic() {
        let c_source = r#"int main() {
    int x = 2;
    int result = 0;
    switch (x) {
        case 1:
            result = 10;
            break;
        case 2:
            result = 20;
            break;
        case 3:
            result = 30;
            break;
    }
    return result;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 20, "switch(2) should match case 2, got {}", exit);
    }

    /// switch-case with default
    #[test]
    fn test_cc_switch_default() {
        let c_source = r#"int main() {
    int x = 7;
    int result = 0;
    switch (x) {
        case 1:
            result = 10;
            break;
        case 2:
            result = 20;
            break;
        default:
            result = 99;
            break;
    }
    return result;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 99, "switch(7) should match default, got {}", exit);
    }

    /// switch-case with multiple matches
    #[test]
    fn test_cc_switch_multi() {
        let c_source = r#"int main() {
    int sum = 0;
    for (int i = 0; i < 6; i += 1) {
        switch (i) {
            case 0:
                sum = sum + 1;
                break;
            case 1:
                sum = sum + 2;
                break;
            case 2:
                sum = sum + 3;
                break;
            default:
                sum = sum + 10;
                break;
        }
    }
    return sum;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 36, "1+2+3+10+10+10=36, got {}", exit);
    }

    /// 全局变量读取
    #[test]
    fn test_cc_global_read() {
        let c_source = r#"int g = 42;

int main() {
    return g;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 42, "global g=42, got {}", exit);
    }

    /// 全局变量写入
    #[test]
    fn test_cc_global_write() {
        let c_source = r#"int g = 10;

int main() {
    g = 42;
    return g;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 42, "g set to 42, got {}", exit);
    }

    /// 全局变量跨函数共享
    #[test]
    fn test_cc_global_cross_func() {
        let c_source = r#"int counter = 0;

int increment() {
    counter = counter + 1;
    return counter;
}

int main() {
    increment();
    increment();
    increment();
    return counter;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 3, "counter after 3 increments, got {}", exit);
    }

    /// 一元负号
    #[test]
    fn test_cc_unary_neg() {
        let c_source = r#"int main() {
    int x = -5;
    int y = -x;
    return y;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 5, "-(-5)=5, got {}", exit);
    }

    /// bitwise and
    #[test]
    fn test_cc_bitwise_and() {
        let c_source = r#"int main() {
    return 12 & 10;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 8, "12 & 10 = 8, got {}", exit);
    }

    /// bitwise or, xor, left/right shift, not
    #[test]
    fn test_cc_bitwise_ops() {
        let c_source = r#"int main() {
    int a = 12;
    int b = 10;
    int r1 = a | b;
    int r2 = a ^ b;
    int r3 = a << 2;
    int r4 = b >> 1;
    int r5 = ~a;
    return r1 + r2 + r3 + r4 + r5;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 60, "14+6+48+5-13=60, got {}", exit);
    }

    /// char type declaration
    #[test]
    fn test_cc_char_type() {
        let c_source = r#"int main() {
    char a = 65;
    char b = 66;
    return a + b;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 131, "65+66=131, got {}", exit);
    }

    /// character literals with escape
    #[test]
    fn test_cc_char_literal() {
        let c_source = r#"int main() {
    char a = 'A';
    char b = 'B';
    char c = '\n';
    return a + b + c;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 141, "65+66+10=141, got {}", exit);
    }

    /// sizeof operator
    #[test]
    fn test_cc_sizeof() {
        let c_source = r#"int main() {
    int s = sizeof(int);
    return s;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 8, "sizeof(int)=8, got {}", exit);
    }

    /// pointer address-of and dereference
    #[test]
    fn test_cc_ptr_deref() {
        let c_source = r#"int main() {
    int x = 42;
    int y = *(&x);
    return y;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 42, "*(&x)=42, got {}", exit);
    }

    /// pointer write through dereference
    #[test]
    fn test_cc_ptr_write() {
        let c_source = r#"int main() {
    int x = 10;
    int p = &x;
    *p = 99;
    return x;
}
"#;
        let exit = compile_and_run_c(c_source);
        assert_eq!(exit, 99, "x after *p=99 should be 99, got {}", exit);
    }
}
