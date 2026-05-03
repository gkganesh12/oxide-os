// oxide-os/kernel/src/serial.rs
//! Serial port (UART 16550) driver for kernel debug output.

use core::fmt;
use core::fmt::Write;
use spin::Mutex;
use uart_16550::SerialPort;

/// Global serial port, protected by a spinlock.
static SERIAL: Mutex<Option<SerialPort>> = Mutex::new(None);

/// Initialize the serial port at COM1 (0x3F8).
pub fn init() {
    let mut port = unsafe { SerialPort::new(0x3F8) };
    port.init();
    *SERIAL.lock() = Some(port);
}

/// Print formatted arguments to the serial port.
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    if let Some(ref mut port) = *SERIAL.lock() {
        port.write_fmt(args).unwrap();
    }
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
