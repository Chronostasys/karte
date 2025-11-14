use std::fmt;

/// IR Display trait - 用于将 IR 类型转换为可读的文本格式
pub trait IrDisplay {
    /// 将值格式化为字符串
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;

    /// 转换为字符串（便捷方法）
    fn to_ir_string(&self) -> String {
        struct DisplayWrapper<'a, T: ?Sized>(&'a T);

        impl<'a, T: IrDisplay + ?Sized> fmt::Display for DisplayWrapper<'a, T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.ir_fmt(f)
            }
        }

        DisplayWrapper(self).to_string()
    }
}

/// 在已经写入前缀内容后，追加一个可能包含多行的值，并对后续行进行缩进。
pub fn write_multiline_suffix<T: IrDisplay>(
    f: &mut fmt::Formatter<'_>,
    value: &T,
    indent: usize,
) -> fmt::Result {
    let rendered = value.to_ir_string();
    let normalized = normalize_indentation(&rendered);

    for line in normalized.lines() {
        write!(f, "\n{:indent$}{}", "", line, indent = indent)?;
    }

    Ok(())
}

const INDENT_STEP: usize = 4;

fn normalize_indentation(block: &str) -> String {
    let mut normalized = String::with_capacity(block.len());
    let mut level = 0usize;

    for raw_line in block.lines() {
        let line = raw_line.trim_end();

        if line.trim().is_empty() {
            normalized.push('\n');
            continue;
        }

        let trimmed = line.trim_start();
        let mut indent_level = level;
        let mut leading_closing = false;

        if let Some(first_char) = trimmed.chars().next() {
            if first_char == '}' || first_char == ']' {
                leading_closing = true;
                if indent_level > 0 {
                    indent_level -= 1;
                }
            }
        }

        for _ in 0..(indent_level * INDENT_STEP) {
            normalized.push(' ');
        }
        normalized.push_str(trimmed);
        normalized.push('\n');

        let mut local_level = indent_level;
        for (idx, ch) in trimmed.chars().enumerate() {
            if leading_closing && idx == 0 {
                continue;
            }
            match ch {
                '{' | '[' => local_level += 1,
                '}' | ']' => {
                    if local_level > 0 {
                        local_level -= 1;
                    }
                }
                _ => {}
            }
        }
        level = local_level;
    }

    while normalized.ends_with('\n') {
        normalized.pop();
    }

    normalized
}

// 为基础类型实现 IrDisplay
impl IrDisplay for String {
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl IrDisplay for &str {
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl IrDisplay for i64 {
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl IrDisplay for usize {
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl IrDisplay for u8 {
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

impl IrDisplay for bool {
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", if *self { "true" } else { "false" })
    }
}

impl<T: IrDisplay> IrDisplay for Option<T> {
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Some(v) => v.ir_fmt(f),
            None => write!(f, "none"),
        }
    }
}

impl<T: IrDisplay> IrDisplay for Box<T> {
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).ir_fmt(f)
    }
}

impl<T: IrDisplay> IrDisplay for Vec<T> {
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return write!(f, "[]");
        }

        // 对于语句列表等，总是使用换行格式以提高可读性
        write!(f, "[\n")?;
        for (i, item) in self.iter().enumerate() {
            write!(f, "    ")?; // 4个空格缩进
            write_multiline_suffix(f, item, 8)?;
            if i + 1 < self.len() {
                write!(f, ",\n")?;
            } else {
                write!(f, "\n")?;
            }
        }
        write!(f, "    ]") // 闭合括号4个空格缩进
    }
}

/// 辅助函数：格式化逗号分隔的列表
pub fn fmt_comma_separated<T: IrDisplay>(items: &[T], f: &mut fmt::Formatter<'_>) -> fmt::Result {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            write!(f, ", ")?;
        }
        item.ir_fmt(f)?;
    }
    Ok(())
}

/// 辅助函数：格式化带括号的参数列表
pub fn fmt_args<T: IrDisplay>(args: &[T], f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "(")?;
    fmt_comma_separated(args, f)?;
    write!(f, ")")
}
