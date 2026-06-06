use super::runtime;
use karte_lir::Register;

/// 运行时固有函数
#[derive(Debug, Clone, Copy)]
pub enum RuntimeIntrinsic {
    AllocAligned,
    Free,
    Retain,
    Release,
    GcSafepoint,
    StringConcat,
    StringEqual,
    StringCompare,
    StringCharAt,
    StringSubstring,
    StringContains,
    SplitCount,
    Trim,
    CharToString,
    ToString,
    PrintString,
    PrintNumber,
    PrintBool,
    Panic,
    MemLoad64,
    MemStore64,
    GcAlloc,
}

impl RuntimeIntrinsic {
    pub fn symbol_ptr(self) -> *const () {
        match self {
            RuntimeIntrinsic::AllocAligned => runtime::karte_jit_runtime_alloc_aligned as *const (),
            RuntimeIntrinsic::Free => runtime::karte_jit_runtime_free as *const (),
            RuntimeIntrinsic::Retain => runtime::karte_jit_runtime_retain as *const (),
            RuntimeIntrinsic::Release => runtime::karte_jit_runtime_release as *const (),
            RuntimeIntrinsic::GcSafepoint => runtime::karte_jit_runtime_gc_safepoint as *const (),
            RuntimeIntrinsic::StringConcat => {
                runtime::karte_jit_runtime_string_concat as *const ()
            }
            RuntimeIntrinsic::StringEqual => {
                runtime::karte_jit_runtime_string_equal as *const ()
            }
            RuntimeIntrinsic::StringCompare => {
                runtime::karte_jit_runtime_string_compare as *const ()
            }
            RuntimeIntrinsic::StringCharAt => {
                runtime::karte_jit_runtime_string_char_at as *const ()
            }
            RuntimeIntrinsic::StringSubstring => {
                runtime::karte_jit_runtime_string_substring as *const ()
            }
            RuntimeIntrinsic::StringContains => {
                runtime::karte_jit_runtime_string_contains as *const ()
            }
            RuntimeIntrinsic::SplitCount => {
                runtime::karte_jit_runtime_split_count as *const ()
            }
            RuntimeIntrinsic::Trim => {
                runtime::karte_jit_runtime_trim as *const ()
            }
            RuntimeIntrinsic::CharToString => {
                runtime::karte_jit_runtime_char_to_string as *const ()
            }
            RuntimeIntrinsic::ToString => {
                runtime::karte_jit_runtime_to_string as *const ()
            }
            RuntimeIntrinsic::PrintString => {
                runtime::karte_jit_runtime_print_string as *const ()
            }
            RuntimeIntrinsic::PrintNumber => {
                runtime::karte_jit_runtime_print_number as *const ()
            }
            RuntimeIntrinsic::PrintBool => {
                runtime::karte_jit_runtime_print_bool as *const ()
            }
            RuntimeIntrinsic::Panic => runtime::karte_jit_runtime_panic as *const (),
            RuntimeIntrinsic::MemLoad64 => {
                runtime::karte_jit_runtime_mem_load64 as *const ()
            }
            RuntimeIntrinsic::MemStore64 => {
                runtime::karte_jit_runtime_mem_store64 as *const ()
            }
            RuntimeIntrinsic::GcAlloc => {
                runtime::karte_jit_runtime_gc_alloc as *const ()
            }
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RuntimeIntrinsic::AllocAligned => "karte_jit_runtime_alloc_aligned",
            RuntimeIntrinsic::Free => "karte_jit_runtime_free",
            RuntimeIntrinsic::Retain => "karte_jit_runtime_retain",
            RuntimeIntrinsic::Release => "karte_jit_runtime_release",
            RuntimeIntrinsic::GcSafepoint => "karte_jit_runtime_gc_safepoint",
            RuntimeIntrinsic::StringConcat => "karte_jit_runtime_string_concat",
            RuntimeIntrinsic::StringEqual => "karte_jit_runtime_string_equal",
            RuntimeIntrinsic::StringCompare => "karte_jit_runtime_string_compare",
            RuntimeIntrinsic::StringCharAt => "karte_jit_runtime_string_char_at",
            RuntimeIntrinsic::StringSubstring => "karte_jit_runtime_string_substring",
            RuntimeIntrinsic::StringContains => "karte_jit_runtime_string_contains",
            RuntimeIntrinsic::SplitCount => "karte_jit_runtime_split_count",
            RuntimeIntrinsic::Trim => "karte_jit_runtime_trim",
            RuntimeIntrinsic::CharToString => "karte_jit_runtime_char_to_string",
            RuntimeIntrinsic::ToString => "karte_jit_runtime_to_string",
            RuntimeIntrinsic::PrintString => "karte_jit_runtime_print_string",
            RuntimeIntrinsic::PrintNumber => "karte_jit_runtime_print_number",
            RuntimeIntrinsic::PrintBool => "karte_jit_runtime_print_bool",
            RuntimeIntrinsic::Panic => "karte_jit_runtime_panic",
            RuntimeIntrinsic::MemLoad64 => "karte_jit_runtime_mem_load64",
            RuntimeIntrinsic::MemStore64 => "karte_jit_runtime_mem_store64",
            RuntimeIntrinsic::GcAlloc => "karte_jit_runtime_gc_alloc",
        }
    }

    pub fn has_result(self) -> bool {
        matches!(
            self,
            RuntimeIntrinsic::AllocAligned | RuntimeIntrinsic::StringConcat | RuntimeIntrinsic::StringEqual | RuntimeIntrinsic::StringCompare | RuntimeIntrinsic::StringCharAt | RuntimeIntrinsic::StringSubstring | RuntimeIntrinsic::StringContains | RuntimeIntrinsic::SplitCount | RuntimeIntrinsic::Trim | RuntimeIntrinsic::CharToString | RuntimeIntrinsic::ToString | RuntimeIntrinsic::MemLoad64 | RuntimeIntrinsic::GcAlloc
        )
    }
}

/// 调用参数描述
#[derive(Debug, Clone)]
pub enum RuntimeArg {
    Immediate(i64),
    Register(Register),
}

/// 整个运行时调用描述
#[derive(Debug, Clone)]
pub struct RuntimeCall {
    pub intrinsic: RuntimeIntrinsic,
    pub args: Vec<RuntimeArg>,
}

impl RuntimeCall {
    pub fn alloc(size: usize, alignment: usize) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::AllocAligned,
            args: vec![
                RuntimeArg::Immediate(size.max(8) as i64),
                RuntimeArg::Immediate(alignment.max(8) as i64),
            ],
        }
    }

    pub fn free(ptr: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::Free,
            args: vec![RuntimeArg::Register(ptr)],
        }
    }

    pub fn retain(ptr: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::Retain,
            args: vec![RuntimeArg::Register(ptr)],
        }
    }

    pub fn release(ptr: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::Release,
            args: vec![RuntimeArg::Register(ptr)],
        }
    }

    pub fn gc_safepoint() -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::GcSafepoint,
            args: vec![],
        }
    }

    pub fn string_concat(left: Register, right: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::StringConcat,
            args: vec![RuntimeArg::Register(left), RuntimeArg::Register(right)],
        }
    }

    pub fn string_equal(left: Register, right: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::StringEqual,
            args: vec![RuntimeArg::Register(left), RuntimeArg::Register(right)],
        }
    }

    pub fn string_compare(left: Register, right: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::StringCompare,
            args: vec![RuntimeArg::Register(left), RuntimeArg::Register(right)],
        }
    }

    pub fn string_char_at(str_reg: Register, index_reg: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::StringCharAt,
            args: vec![RuntimeArg::Register(str_reg), RuntimeArg::Register(index_reg)],
        }
    }

    pub fn string_substring(str_reg: Register, start_reg: Register, length_reg: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::StringSubstring,
            args: vec![RuntimeArg::Register(str_reg), RuntimeArg::Register(start_reg), RuntimeArg::Register(length_reg)],
        }
    }

    pub fn string_contains(str_reg: Register, char_code_reg: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::StringContains,
            args: vec![RuntimeArg::Register(str_reg), RuntimeArg::Register(char_code_reg)],
        }
    }

    pub fn split_count(str_reg: Register, sep_reg: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::SplitCount,
            args: vec![RuntimeArg::Register(str_reg), RuntimeArg::Register(sep_reg)],
        }
    }

    pub fn trim(str_reg: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::Trim,
            args: vec![RuntimeArg::Register(str_reg)],
        }
    }

    pub fn char_to_string(value: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::CharToString,
            args: vec![RuntimeArg::Register(value)],
        }
    }

    pub fn gc_alloc(size: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::GcAlloc,
            args: vec![RuntimeArg::Register(size)],
        }
    }

    pub fn mem_load64(addr: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::MemLoad64,
            args: vec![RuntimeArg::Register(addr)],
        }
    }

    pub fn mem_store64(addr: Register, value: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::MemStore64,
            args: vec![RuntimeArg::Register(addr), RuntimeArg::Register(value)],
        }
    }

    pub fn to_string(value: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::ToString,
            args: vec![RuntimeArg::Register(value)],
        }
    }

    pub fn print_string(ptr: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::PrintString,
            args: vec![RuntimeArg::Register(ptr)],
        }
    }

    pub fn print_number(value: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::PrintNumber,
            args: vec![RuntimeArg::Register(value)],
        }
    }

    pub fn print_bool(value: Register) -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::PrintBool,
            args: vec![RuntimeArg::Register(value)],
        }
    }

    pub fn panic() -> Self {
        Self {
            intrinsic: RuntimeIntrinsic::Panic,
            args: vec![],
        }
    }

    pub fn expects_result(&self) -> bool {
        self.intrinsic.has_result()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retain_release_calls_have_single_register_arg() {
        let dummy = Register::Virtual(42);
        let retain = RuntimeCall::retain(dummy);
        assert!(matches!(retain.intrinsic, RuntimeIntrinsic::Retain));
        assert_eq!(retain.args.len(), 1);

        let release = RuntimeCall::release(dummy);
        assert!(matches!(release.intrinsic, RuntimeIntrinsic::Release));
        assert_eq!(release.args.len(), 1);
    }
}
