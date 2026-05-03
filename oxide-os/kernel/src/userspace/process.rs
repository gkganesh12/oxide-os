use alloc::string::String;
use alloc::vec::Vec;
use crate::task::TaskId;
use crate::capability::CapId;
use crate::println;

/// User-space process with its own address space.
#[derive(Debug)]
pub struct Process {
    pub pid: TaskId,
    pub name: String,
    pub capabilities: Vec<CapId>,
    pub entry_point: u64,
    /// User-mode stack top virtual address.
    pub user_stack_top: u64,
}

/// The GDT segment selectors for user mode.
/// These must match the GDT entries in gdt.rs.
pub const USER_CODE_SELECTOR: u16 = 0x1B; // Ring 3, index 3
pub const USER_DATA_SELECTOR: u16 = 0x23; // Ring 3, index 4

impl Process {
    pub fn new(pid: TaskId, name: String, entry_point: u64) -> Self {
        Process {
            pid,
            name,
            capabilities: Vec::new(),
            entry_point,
            user_stack_top: 0x0000_7FFF_FFFF_F000, // Below kernel half
        }
    }

    /// Prepare to jump to user mode. Sets up the stack frame for `iretq`.
    /// This would be called after mapping the user binary + stack pages.
    ///
    /// The iretq frame on the stack (pushed in reverse order):
    /// - SS (user data selector)
    /// - RSP (user stack pointer)
    /// - RFLAGS (interrupts enabled)
    /// - CS (user code selector)
    /// - RIP (entry point)
    pub fn enter_usermode(&self) -> ! {
        println!("[process] Entering ring-3 for '{}' at RIP={:#X}", self.name, self.entry_point);
        unsafe {
            core::arch::asm!(
                "cli",                          // Disable interrupts during transition
                "mov ax, {user_ds:x}",          // Load user data segment
                "mov ds, ax",
                "mov es, ax",
                "mov fs, ax",
                "mov gs, ax",
                // Build iretq frame on stack
                "push {user_ss}",              // SS
                "push {user_rsp}",             // RSP
                "push 0x200",                  // RFLAGS (IF=1, interrupts enabled)
                "push {user_cs}",              // CS
                "push {entry}",                // RIP
                "iretq",                       // Switch to ring 3
                user_ds = in(reg) USER_DATA_SELECTOR as u64,
                user_ss = in(reg) USER_DATA_SELECTOR as u64,
                user_rsp = in(reg) self.user_stack_top,
                user_cs = in(reg) USER_CODE_SELECTOR as u64,
                entry = in(reg) self.entry_point,
                options(noreturn),
            );
        }
    }
}
