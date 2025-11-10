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
    
    // 让所有行都从新行开始,使用相同的缩进
    // 这样解析器就只需要识别换行和关键字,不依赖空格数量
    for line in rendered.lines() {
        write!(f, "\n{:indent$}{}", "", line, indent = indent)?;
    }

    Ok(())
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
