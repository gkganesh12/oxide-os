//! CPU context save/restore for context switching.

/// Saved callee-saved registers for context switching.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct CpuContext {
    pub rsp: u64,
    pub rbp: u64,
    pub rbx: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
}

impl CpuContext {
    pub const fn empty() -> Self {
        CpuContext {
            rsp: 0, rbp: 0, rbx: 0, r12: 0,
            r13: 0, r14: 0, r15: 0, rip: 0,
        }
    }
}

/// Switch from `old` context to `new` context.
/// Saves callee-saved registers to `old`, restores from `new`, jumps to new.rip.
///
/// Safety: both pointers must be valid CpuContext structs.
#[unsafe(naked)]
pub unsafe extern "C" fn context_switch(_old: *mut CpuContext, _new: *const CpuContext) {
    core::arch::naked_asm!(
        // Save callee-saved registers to old (rdi)
        "mov [rdi + 0x00], rsp",
        "mov [rdi + 0x08], rbp",
        "mov [rdi + 0x10], rbx",
        "mov [rdi + 0x18], r12",
        "mov [rdi + 0x20], r13",
        "mov [rdi + 0x28], r14",
        "mov [rdi + 0x30], r15",
        // Save return address as rip
        "lea rax, [rip + 2f]",
        "mov [rdi + 0x38], rax",
        // Restore from new (rsi)
        "mov rsp, [rsi + 0x00]",
        "mov rbp, [rsi + 0x08]",
        "mov rbx, [rsi + 0x10]",
        "mov r12, [rsi + 0x18]",
        "mov r13, [rsi + 0x20]",
        "mov r14, [rsi + 0x28]",
        "mov r15, [rsi + 0x30]",
        // Re-enable interrupts (we left the interrupt handler without iretq)
        "sti",
        // Jump to new task's rip
        "jmp [rsi + 0x38]",
        // Return label — when we switch BACK to the old task, we land here
        "2:",
        "ret",
    );
}
