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
    PrintString,
    PrintNumber,
    PrintBool,
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
            RuntimeIntrinsic::PrintString => {
                runtime::karte_jit_runtime_print_string as *const ()
            }
            RuntimeIntrinsic::PrintNumber => {
                runtime::karte_jit_runtime_print_number as *const ()
            }
            RuntimeIntrinsic::PrintBool => {
                runtime::karte_jit_runtime_print_bool as *const ()
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
            RuntimeIntrinsic::PrintString => "karte_jit_runtime_print_string",
            RuntimeIntrinsic::PrintNumber => "karte_jit_runtime_print_number",
            RuntimeIntrinsic::PrintBool => "karte_jit_runtime_print_bool",
        }
    }

    pub fn has_result(self) -> bool {
        matches!(
            self,
            RuntimeIntrinsic::AllocAligned | RuntimeIntrinsic::StringConcat | RuntimeIntrinsic::StringEqual
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
