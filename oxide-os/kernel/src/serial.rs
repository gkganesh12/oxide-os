//! Serial port (UART 16550) driver for kernel debug output.

use core::fmt;
use core::fmt::Write;
use spin::Mutex;
use uart_16550::SerialPort;
use x86_64::instructions::interrupts;

/// Global serial port, protected by a spinlock.
static SERIAL: Mutex<Option<SerialPort>> = Mutex::new(None);

/// Initialize the serial port at COM1 (0x3F8).
pub fn init() {
    let mut port = unsafe { SerialPort::new(0x3F8) };
    port.init();
    *SERIAL.lock() = Some(port);
}

/// Print formatted arguments to the serial port.
/// Disables interrupts to prevent deadlocks if an interrupt handler prints.
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    interrupts::without_interrupts(|| {
        if let Some(ref mut port) = *SERIAL.lock() {
            port.write_fmt(args).unwrap();
        }
    });
}

/// Force-print without locking (for use in panic handler only).
/// Safety: only call when you know the system is in a fatal state.
pub fn _panic_print(args: fmt::Arguments) {
    // Bypass the lock — we're dying anyway, better to get output than deadlock
    let mut port = unsafe { SerialPort::new(0x3F8) };
    port.init();
    port.write_fmt(args).ok();
}

/// Print to the serial console.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::serial::_print(format_args!($($arg)*)));
}

/// Print to the serial console, with a newline.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

/// Print in panic context (bypasses lock to avoid deadlock).
#[macro_export]
macro_rules! panic_println {
    () => ($crate::serial::_panic_print(format_args!("\n")));
    ($($arg:tt)*) => ($crate::serial::_panic_print(format_args!("{}\n", format_args!($($arg)*))));
}
