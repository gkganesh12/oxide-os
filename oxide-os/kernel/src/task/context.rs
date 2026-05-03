//! CPU context save/restore for context switching.
//!
//! Context switching in Oxide OS works at the function-call level, not the
//! interrupt-frame level. The timer interrupt sets a "need_reschedule" flag,
//! and the actual switch happens after the interrupt returns cleanly via iretq.
//! This avoids leaking interrupt frames on the stack.

/// Saved callee-saved registers for context switching.
/// Layout must match the assembly in `context_switch` exactly.
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

/// Switch from current execution context to `new` context, saving current to `old`.
///
/// This is a callee-saved register switch. It:
/// 1. Saves RBP, RBX, R12-R15, RSP, and return address to `old`
/// 2. Restores the same from `new`
/// 3. "Returns" to the new task's saved RIP
///
/// When the old task is switched back to, execution resumes right after the
/// original call to `context_switch` (at label 2).
///
/// Safety:
/// - Both pointers must point to valid, aligned CpuContext structs
/// - The new context's RSP must point to a valid, mapped stack
/// - Must be called with interrupts DISABLED (caller's responsibility)
#[unsafe(naked)]
pub unsafe extern "C" fn context_switch(_old: *mut CpuContext, _new: *const CpuContext) {
    core::arch::naked_asm!(
        // Save callee-saved registers to old context (rdi = old)
        "mov [rdi + 0x00], rsp",
        "mov [rdi + 0x08], rbp",
        "mov [rdi + 0x10], rbx",
        "mov [rdi + 0x18], r12",
        "mov [rdi + 0x20], r13",
        "mov [rdi + 0x28], r14",
        "mov [rdi + 0x30], r15",
        // Save the return address (where caller will resume) as RIP
        "lea rax, [rip + 2f]",
        "mov [rdi + 0x38], rax",

        // Restore callee-saved registers from new context (rsi = new)
        "mov rsp, [rsi + 0x00]",
        "mov rbp, [rsi + 0x08]",
        "mov rbx, [rsi + 0x10]",
        "mov r12, [rsi + 0x18]",
        "mov r13, [rsi + 0x20]",
        "mov r14, [rsi + 0x28]",
        "mov r15, [rsi + 0x30]",

        // Jump to the new task's saved RIP
        // For a fresh task, this is the entry function.
        // For a previously-switched task, this is label 2 below.
        "jmp [rsi + 0x38]",

        // Resume point — when this task is switched BACK to, we land here.
        "2:",
        "ret",
    );
}
