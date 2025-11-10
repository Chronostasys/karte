/// 专门为 BTreeMap 提供更好的多行显示
use std::collections::BTreeMap;
use std::fmt;

/// 为 BTreeMap 提供带缩进的多行显示
pub fn fmt_btreemap_multiline<K, V, F>(
    map: &BTreeMap<K, V>,
    f: &mut fmt::Formatter,
    indent: usize,
) -> fmt::Result
where
    K: fmt::Display + Ord,
    V: crate::IrDisplay,
{
    if map.is_empty() {
        return write!(f, "{{}}");
    }
    
    writeln!(f, "{{")?;
    for (i, (k, v)) in map.iter().enumerate() {
        write!(f, "{:indent$}{}: ", "", k, indent = indent + 2)?;
        crate::IrDisplay::ir_fmt(v, f)?;
        if i < map.len() - 1 {
            writeln!(f)?;
        }
    }
    write!(f, "\n{:indent$}}}", "", indent = indent)
}

