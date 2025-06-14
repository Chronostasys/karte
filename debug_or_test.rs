use std::process::Command;

fn main() {
    // 创建一个简单的Karte程序来测试逻辑OR
    let program = "true || false";
    
    // 将程序写入临时文件
    std::fs::write("temp_or_test.karte", program).unwrap();
    
    // 使用karte命令行工具执行，启用调试模式
    let output = Command::new("./target/debug/karte")
        .args(&["temp_or_test.karte", "--debug"])
        .output()
        .expect("Failed to execute karte");
    
    println!("Exit status: {}", output.status);
    println!("Stdout:\n{}", String::from_utf8_lossy(&output.stdout));
    println!("Stderr:\n{}", String::from_utf8_lossy(&output.stderr));
    
    // 清理临时文件
    std::fs::remove_file("temp_or_test.karte").ok();
} 