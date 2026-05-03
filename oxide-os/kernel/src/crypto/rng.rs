use spin::Mutex;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::println;

static HAS_RDRAND: AtomicBool = AtomicBool::new(false);
static FALLBACK: Mutex<u64> = Mutex::new(0xDEAD_BEEF_CAFE_BABE);

pub fn init() {
    // Check CPUID for RDRAND (ECX bit 30 of leaf 1)
    let ecx: u32;
    unsafe {
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "pop rbx",
            inout("eax") 1u32 => _,
            lateout("ecx") ecx,
            out("edx") _,
        );
    }
    let has_hw = (ecx >> 30) & 1 == 1;
    HAS_RDRAND.store(has_hw, Ordering::Release);

    if !has_hw {
        // Seed from TSC
        let lo: u32;
        let hi: u32;
        unsafe {
            core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi);
        }
        let tsc = ((hi as u64) << 32) | (lo as u64);
        *FALLBACK.lock() = if tsc == 0 { 0xDEAD_BEEF } else { tsc };
    }
    println!("[crypto] RNG: {}", if has_hw { "RDRAND (hardware)" } else { "XorShift64 (software)" });
}

pub fn random_u64() -> u64 {
    if HAS_RDRAND.load(Ordering::Acquire) {
        for _ in 0..10 {
            let val: u64;
            let ok: u8;
            unsafe {
                core::arch::asm!("rdrand {v}", "setc {ok}", v = out(reg) val, ok = out(reg_byte) ok);
            }
            if ok == 1 {
                return val;
            }
        }
    }
    // Fallback: XorShift64
    let mut state = FALLBACK.lock();
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

pub fn fill_bytes(buffer: &mut [u8]) {
    let mut i = 0;
    while i < buffer.len() {
        let val = random_u64();
        let bytes = val.to_le_bytes();
        let remaining = buffer.len() - i;
        let to_copy = remaining.min(8);
        buffer[i..i + to_copy].copy_from_slice(&bytes[..to_copy]);
        i += to_copy;
    }
}
