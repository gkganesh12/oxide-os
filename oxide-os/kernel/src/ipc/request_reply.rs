use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;
use super::{IpcError, MessageId, MessageType, message};
use crate::task::TaskId;
use crate::task::scheduler::SCHEDULER;
use crate::capability::{CapId, CAP_TABLE, PermissionBits};
use crate::interrupts;

struct PendingRequest {
    requester: TaskId,
    deadline_tick: u64,
}

static PENDING: Mutex<BTreeMap<u64, PendingRequest>> = Mutex::new(BTreeMap::new());
static REPLIES: Mutex<BTreeMap<u64, message::Message>> = Mutex::new(BTreeMap::new());

/// Send a request and register for reply. Caller should block after this.
pub fn request(
    sender: TaskId,
    recipient: TaskId,
    payload: Vec<u8>,
    sender_cap: CapId,
    timeout_ticks: u64,
) -> Result<MessageId, IpcError> {
    // Validate capability
    {
        let table = CAP_TABLE.lock();
        table.validate(sender_cap, sender, PermissionBits::WRITE)
            .map_err(|_| IpcError::CapabilityDenied)?;
    }

    let msg_id = message::send(sender, recipient, MessageType::Request, payload, None, None, sender_cap)?;
    let deadline = interrupts::ticks() + timeout_ticks;

    PENDING.lock().insert(msg_id.0, PendingRequest { requester: sender, deadline_tick: deadline });

    Ok(msg_id)
}

/// Send a reply to a pending request.
pub fn reply(
    sender: TaskId,
    original_msg_id: MessageId,
    payload: Vec<u8>,
    sender_cap: CapId,
) -> Result<(), IpcError> {
    let pending = PENDING.lock().remove(&original_msg_id.0)
        .ok_or(IpcError::RecipientNotFound)?;

    let reply_msg = message::Message {
        id: MessageId(0),
        sender,
        recipient: pending.requester,
        msg_type: MessageType::Reply,
        payload,
        cap_transfer: None,
        reply_to: Some(original_msg_id),
    };

    REPLIES.lock().insert(original_msg_id.0, reply_msg);

    // Wake the requester
    if let Some(mut sched) = SCHEDULER.try_lock() {
        sched.unblock(pending.requester);
    }

    Ok(())
}

/// Collect a reply (called by requester after being woken).
pub fn collect_reply(msg_id: MessageId) -> Option<message::Message> {
    REPLIES.lock().remove(&msg_id.0)
}

/// Check for timed-out requests. Called from timer interrupt — uses try_lock
/// to avoid deadlock if PENDING is held by a task.
pub fn check_timeouts() {
    let mut pending = match PENDING.try_lock() {
        Some(p) => p,
        None => return, // Lock held, skip this check
    };

    let current_tick = interrupts::ticks();

    let expired: Vec<u64> = pending.iter()
        .filter(|(_, req)| current_tick >= req.deadline_tick)
        .map(|(id, _)| *id)
        .collect();

    for id in expired {
        if let Some(req) = pending.remove(&id) {
            if let Some(mut sched) = SCHEDULER.try_lock() {
                sched.unblock(req.requester);
            }
        }
    }
}
