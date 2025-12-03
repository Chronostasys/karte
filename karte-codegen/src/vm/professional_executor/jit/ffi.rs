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
}

impl RuntimeIntrinsic {
    pub fn symbol_ptr(self) -> *const () {
        match self {
            RuntimeIntrinsic::AllocAligned => runtime::karte_jit_runtime_alloc_aligned as *const (),
            RuntimeIntrinsic::Free => runtime::karte_jit_runtime_free as *const (),
            RuntimeIntrinsic::Retain => runtime::karte_jit_runtime_retain as *const (),
            RuntimeIntrinsic::Release => runtime::karte_jit_runtime_release as *const (),
            RuntimeIntrinsic::GcSafepoint => runtime::karte_jit_runtime_gc_safepoint as *const (),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            RuntimeIntrinsic::AllocAligned => "karte_jit_runtime_alloc_aligned",
            RuntimeIntrinsic::Free => "karte_jit_runtime_free",
            RuntimeIntrinsic::Retain => "karte_jit_runtime_retain",
            RuntimeIntrinsic::Release => "karte_jit_runtime_release",
            RuntimeIntrinsic::GcSafepoint => "karte_jit_runtime_gc_safepoint",
        }
    }

    pub fn has_result(self) -> bool {
        matches!(self, RuntimeIntrinsic::AllocAligned)
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
