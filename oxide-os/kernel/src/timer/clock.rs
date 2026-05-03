use core::sync::atomic::{AtomicU64, Ordering};
use crate::println;

static BOOT_TSC: AtomicU64 = AtomicU64::new(0);
// Approximate ticks per second (calibrated at boot)
static TICKS_PER_SEC: AtomicU64 = AtomicU64::new(100);

pub fn init() {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi);
    }
    let tsc = ((hi as u64) << 32) | (lo as u64);
    BOOT_TSC.store(tsc, Ordering::Release);
    println!("[timer] System clock initialized (TSC at boot: {})", tsc);
}

pub fn ticks() -> u64 {
    crate::interrupts::ticks()
}

pub fn millis() -> u64 {
    ticks() * 1000 / TICKS_PER_SEC.load(Ordering::Relaxed).max(1)
}

pub fn seconds() -> u64 {
    ticks() / TICKS_PER_SEC.load(Ordering::Relaxed).max(1)
}

pub fn ms_to_ticks(ms: u64) -> u64 {
    ms * TICKS_PER_SEC.load(Ordering::Relaxed) / 1000
}
