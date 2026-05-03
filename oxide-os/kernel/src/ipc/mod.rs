pub mod message;
pub mod shared_memory;
pub mod channel;

use crate::task::TaskId;
use crate::capability::CapId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Data,
    Request,
    Reply,
    CapTransfer,
    Signal,
}

#[derive(Debug)]
pub enum IpcError {
    RecipientNotFound,
    MailboxFull,
    NoMessage,
    Timeout,
    CapabilityDenied,
    InvalidCapTransfer,
}
