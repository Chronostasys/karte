//! JIT 运行时辅助函数
//!
//! 该模块仅保留对 `karte-rt` 运行时 FFI 的再导出，便于历史代码逐步迁移。

pub use karte_rt::ffi::{
    karte_jit_runtime_alloc, karte_jit_runtime_alloc_aligned, karte_jit_runtime_free,
    karte_jit_runtime_gc_safepoint, karte_jit_runtime_heap_stats, karte_jit_runtime_print_bool,
    karte_jit_runtime_print_number, karte_jit_runtime_print_string, karte_jit_runtime_panic,
    karte_jit_runtime_release,
    karte_jit_runtime_retain, karte_jit_runtime_string_char_at, karte_jit_runtime_string_concat,
    karte_jit_runtime_string_contains, karte_jit_runtime_string_equal, karte_jit_runtime_string_substring,
    karte_jit_runtime_split_count,
    karte_jit_runtime_char_to_string,
    karte_jit_runtime_trim,
    karte_jit_runtime_to_string,
    karte_jit_runtime_update_stack_top,
};
