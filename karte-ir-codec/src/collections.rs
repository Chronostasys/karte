/// 为标准库集合类型实现 IrDisplay 和 IrParse
use crate::{display::write_multiline_suffix, IrDisplay, IrParse, ParseError, ParseResult};
use nom::{
    character::complete::char,
    combinator::map,
    multi::separated_list0,
    sequence::{delimited, separated_pair, tuple},
    IResult,
};
use std::collections::{BTreeMap, HashMap};
use std::fmt;

// ============================================================================
// BTreeMap
// ============================================================================

impl<K, V> IrDisplay for BTreeMap<K, V>
where
    K: IrDisplay + Ord,
    V: IrDisplay,
{
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return write!(f, "{{}}");
        }

        write!(f, "{{\n")?;
        let len = self.len();
        for (i, (k, v)) in self.iter().enumerate() {
            write!(f, "    ")?; // 4个空格缩进
            k.ir_fmt(f)?;
            write!(f, ": ")?;
            write_multiline_suffix(f, v, 8)?;
            if i + 1 < len {
                write!(f, ",\n")?; // 单换行
            } else {
                write!(f, "\n")?;
            }
        }
        write!(f, "    }}") // 闭合括号对齐缩进
    }
}

impl<K, V> IrParse for BTreeMap<K, V>
where
    K: IrParse + Ord,
    V: IrParse,
{
    fn parse_ir(input: &str) -> ParseResult<Self> {
        let (_, result) = Self::parse_nom(input).map_err(ParseError::from)?;
        Ok(result)
    }

    fn parse_nom(input: &str) -> IResult<&str, Self> {
        use crate::parse::ws;
        use nom::character::complete::{char as nom_char, multispace0};
        use nom::sequence::delimited as nom_delimited;

        log::trace!("Parsing BTreeMap: input = {}", input);

        map(
            nom_delimited(
                ws(nom_char('{')),
                separated_list0(
                    ws(nom_char(',')),
                    // key: value 其中冒号前后允许任意空白
                    separated_pair(
                        K::parse_nom, 
                        nom_delimited(multispace0, nom_char(':'), multispace0),  // 冒号前后允许任意空白
                        V::parse_nom
                    ),
                ),
                ws(nom_char('}')),
            ),
            |pairs| pairs.into_iter().collect(),
        )(input)
    }
}

// ============================================================================
// HashMap
// ============================================================================

impl<K, V> IrDisplay for HashMap<K, V>
where
    K: IrDisplay + std::hash::Hash + Eq,
    V: IrDisplay,
{
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return write!(f, "{{}}");
        }

        write!(f, "{{\n")?;
        let len = self.len();
        for (i, (k, v)) in self.iter().enumerate() {
            write!(f, "    ")?; // 4个空格缩进
            k.ir_fmt(f)?;
            write!(f, ": ")?;
            write_multiline_suffix(f, v, 8)?;
            if i + 1 < len {
                write!(f, ",\n\n")?; // 函数之间保持双换行
            } else {
                write!(f, "\n")?;
            }
        }
        write!(f, "    }}") // 闭合括号对齐缩进
    }
}

impl<K, V> IrParse for HashMap<K, V>
where
    K: IrParse + std::hash::Hash + Eq,
    V: IrParse,
{
    fn parse_ir(input: &str) -> ParseResult<Self> {
        let (_, result) = Self::parse_nom(input).map_err(ParseError::from)?;
        Ok(result)
    }

    fn parse_nom(input: &str) -> IResult<&str, Self> {
        use crate::parse::ws;
        use nom::character::complete::{char as nom_char, multispace0};
        use nom::sequence::delimited as nom_delimited;

        
        // 测试解析 key-value pair 的辅助函数
        fn test_parse_pair<'a, K: IrParse, V: IrParse>(input: &'a str) -> IResult<&'a str, (K, V)> {
            log::trace!("→ Parsing key-value pair from: {:?}", &input.chars().take(80).collect::<String>());
            
            let (rest, key) = K::parse_nom(input)?;
            log::trace!("  ✓ Parsed key, rest: {:?}", &rest.chars().take(60).collect::<String>());
            
            let (rest, _) = nom_delimited(multispace0, nom_char(':'), multispace0)(rest)?;
            log::trace!("  ✓ Parsed colon, rest: {:?}", &rest.chars().take(60).collect::<String>());
            
            match V::parse_nom(rest) {
                Ok((final_rest, value)) => {
                    log::trace!("  ✓ Parsed value, rest: {:?}", &final_rest.chars().take(60).collect::<String>());
                    Ok((final_rest, (key, value)))
                }
                Err(e) => {
                    log::trace!("  ✗ Failed to parse value: {:?}", e);
                    Err(e)
                }
            }
        }
        
        let (input_after_brace, _) = ws(nom_char('{'))(input)?;
        log::trace!("After opening brace, input: {:?}", &input_after_brace.chars().take(100).collect::<String>());
        
        let (input_after_list, pairs) = separated_list0(
            ws(nom_char(',')),
            |input| test_parse_pair::<K, V>(input)
        )(input_after_brace)?;
        log::trace!("After parsing list, input: {:?}", &input_after_list.chars().take(100).collect::<String>());
        
        let (final_input, _) = ws(nom_char('}'))(input_after_list)?;
        log::trace!("After closing brace, input: {:?}", &final_input.chars().take(60).collect::<String>());
        
        Ok((final_input, pairs.into_iter().collect()))
    }
}

// ============================================================================
// 元组类型 (Tuple2)
// ============================================================================

impl<T1, T2> IrDisplay for (T1, T2)
where
    T1: IrDisplay,
    T2: IrDisplay,
{
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(")?;
        self.0.ir_fmt(f)?;
        write!(f, ", ")?;
        self.1.ir_fmt(f)?;
        write!(f, ")")
    }
}

impl<T1, T2> IrParse for (T1, T2)
where
    T1: IrParse,
    T2: IrParse,
{
    fn parse_ir(input: &str) -> ParseResult<Self> {
        let (_, result) = Self::parse_nom(input).map_err(ParseError::from)?;
        Ok(result)
    }

    fn parse_nom(input: &str) -> IResult<&str, Self> {
        use crate::parse::ws;

        delimited(
            ws(char('(')),
            map(
                tuple((T1::parse_nom, ws(char(',')), T2::parse_nom)),
                |(t1, _, t2)| (t1, t2),
            ),
            ws(char(')')),
        )(input)
    }
}

// ============================================================================
// 元组类型 (Tuple3)
// ============================================================================

impl<T1, T2, T3> IrDisplay for (T1, T2, T3)
where
    T1: IrDisplay,
    T2: IrDisplay,
    T3: IrDisplay,
{
    fn ir_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(")?;
        self.0.ir_fmt(f)?;
        write!(f, ", ")?;
        self.1.ir_fmt(f)?;
        write!(f, ", ")?;
        self.2.ir_fmt(f)?;
        write!(f, ")")
    }
}

impl<T1, T2, T3> IrParse for (T1, T2, T3)
where
    T1: IrParse,
    T2: IrParse,
    T3: IrParse,
{
    fn parse_ir(input: &str) -> ParseResult<Self> {
        let (_, result) = Self::parse_nom(input).map_err(ParseError::from)?;
        Ok(result)
    }

    fn parse_nom(input: &str) -> IResult<&str, Self> {
        use crate::parse::ws;

        delimited(
            ws(char('(')),
            map(
                tuple((
                    T1::parse_nom,
                    ws(char(',')),
                    T2::parse_nom,
                    ws(char(',')),
                    T3::parse_nom,
                )),
                |(t1, _, t2, _, t3)| (t1, t2, t3),
            ),
            ws(char(')')),
        )(input)
    }
}
