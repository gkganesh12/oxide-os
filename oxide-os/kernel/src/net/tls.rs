//! TLS 1.3 stub — provides the interface for HTTPS.
//! Real TLS requires a crypto library (rustls or custom).
//! For now, this logs TLS handshake attempts and documents what's needed.

use crate::println;
use crate::task::TaskId;
use crate::capability::CapId;

#[derive(Debug)]
pub enum TlsError {
    HandshakeNotImplemented,
    CertificateValidationNotImplemented,
    NoTlsLibrary,
}

/// Attempt a TLS connection. Currently logs and returns an error
/// explaining what's needed for real TLS.
pub fn connect(host: &str, port: u16, task_id: TaskId, _cap_id: CapId) -> Result<(), TlsError> {
    println!("[tls] TLS handshake requested: {}:{} (task {})", host, port, task_id);
    println!("[tls] TLS 1.3 requires: rustls crate + webpki for certificate validation");
    println!("[tls] Current status: TCP connection works, TLS layer pending");
    Err(TlsError::HandshakeNotImplemented)
}

/// Check if TLS is available.
pub fn is_available() -> bool {
    false
}
