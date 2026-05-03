pub mod numbers;
pub mod handler;

use crate::println;

pub fn init() {
    setup_syscall_msrs();
    println!("[syscall] Syscall interface initialized ({} calls registered)", numbers::SYSCALL_COUNT);
}

fn setup_syscall_msrs() {
    use x86_64::registers::model_specific::{Efer, EferFlags, LStar, SFMask};
    use x86_64::registers::rflags::RFlags;
    use x86_64::VirtAddr;

    unsafe {
        // Enable SCE (System Call Extensions) in EFER
        Efer::update(|flags| *flags |= EferFlags::SYSTEM_CALL_EXTENSIONS);

        // Set LSTAR - the RIP the CPU jumps to on `syscall`
        LStar::write(VirtAddr::new(syscall_entry as u64));

        // Mask interrupts on syscall entry
        SFMask::write(RFlags::INTERRUPT_FLAG);
    }

    // Set STAR - kernel CS/SS in bits 47:32, user CS base in bits 63:48
    // STAR[47:32] = kernel CS (0x08), STAR[63:48] = user CS base (0x13)
    // Note: sysret sets CS = STAR[63:48]+16, SS = STAR[63:48]+8 for 64-bit mode
    let star_value: u64 = (0x08u64 << 32) | (0x13u64 << 48);
    unsafe {
        // MSR 0xC0000081 = IA32_STAR
        core::arch::asm!(
            "wrmsr",
            in("ecx") 0xC0000081u32,
            in("eax") star_value as u32,
            in("edx") (star_value >> 32) as u32,
        );
    }

    crate::println!("[syscall] MSRs configured (EFER.SCE, LSTAR, STAR, SFMASK)");
}

/// Syscall entry - the CPU jumps here on `syscall` instruction.
/// We save user state, call the Rust dispatcher, and return via `sysretq`.
///
/// On entry (set by CPU):
///   RCX = user RIP
///   R11 = user RFLAGS
/// On entry (set by user-mode code, Linux convention):
///   RAX = syscall number
///   RDI = arg1, RSI = arg2, RDX = arg3, R10 = arg4, R8 = arg5, R9 = arg6
#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    core::arch::naked_asm!(
        // Save user registers
        "push rcx",         // User RIP (saved by syscall instruction)
        "push r11",         // User RFLAGS (saved by syscall instruction)

        // Save callee-saved regs
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        // Rearrange for Rust ABI call to handler::dispatch(number, a1, a2, a3, a4, a5)
        // On entry: RAX=number, RDI=a1, RSI=a2, RDX=a3, R10=a4, R8=a5
        // Rust ABI wants: RDI=number, RSI=a1, RDX=a2, RCX=a3, R8=a4, R9=a5
        "mov r9, r8",       // a5
        "mov r8, r10",      // a4
        "mov rcx, rdx",     // a3
        "mov rdx, rsi",     // a2
        "mov rsi, rdi",     // a1
        "mov rdi, rax",     // number

        // Call the Rust dispatcher
        "call {dispatch}",

        // Return value is in RAX

        // Restore callee-saved regs
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",

        // Restore user RIP and RFLAGS
        "pop r11",
        "pop rcx",

        // Return to user mode
        "sysretq",
        dispatch = sym crate::syscall::handler::dispatch,
    );
}
