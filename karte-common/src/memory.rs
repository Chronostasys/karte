use karte_ir_derive::IrCodec;

/// Ownership model for heap allocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IrCodec)]
pub enum OwnershipKind {
    /// Caller must manually manage the allocation lifetime (e.g., `free`).
    Manual,
    /// Allocation is managed via reference counting (retain/release hooks).
    RefCounted,
}
