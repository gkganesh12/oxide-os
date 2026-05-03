use alloc::collections::VecDeque;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicU64, Ordering};
use super::{IpcError, MessageId, MessageType};
use crate::task::TaskId;
use crate::capability::{CapId, CAP_TABLE, PermissionBits};

const MAILBOX_CAPACITY: usize = 256;

static NEXT_MSG_ID: AtomicU64 = AtomicU64::new(1);

fn next_message_id() -> MessageId {
    MessageId(NEXT_MSG_ID.fetch_add(1, Ordering::Relaxed))
}

#[derive(Debug, Clone)]
pub struct Message {
    pub id: MessageId,
    pub sender: TaskId,
    pub recipient: TaskId,
    pub msg_type: MessageType,
    pub payload: Vec<u8>,
    pub cap_transfer: Option<CapId>,
    pub reply_to: Option<MessageId>,
}

pub(super) struct Mailbox {
    queue: VecDeque<Message>,
}

impl Mailbox {
    fn new() -> Self {
        Mailbox { queue: VecDeque::with_capacity(16) }
    }

    pub(super) fn push(&mut self, msg: Message) -> Result<(), IpcError> {
        if self.queue.len() >= MAILBOX_CAPACITY {
            return Err(IpcError::MailboxFull);
        }
        self.queue.push_back(msg);
        Ok(())
    }

    fn pop(&mut self) -> Option<Message> {
        self.queue.pop_front()
    }

    fn len(&self) -> usize {
        self.queue.len()
    }
}

pub(super) static MAILBOXES: Mutex<BTreeMap<TaskId, Mailbox>> = Mutex::new(BTreeMap::new());

pub fn register_mailbox(task_id: TaskId) {
    MAILBOXES.lock().insert(task_id, Mailbox::new());
}

pub fn unregister_mailbox(task_id: TaskId) {
    MAILBOXES.lock().remove(&task_id);
}

/// Send a message. Validates sender's capability to communicate with recipient.
pub fn send(
    sender: TaskId,
    recipient: TaskId,
    msg_type: MessageType,
    payload: Vec<u8>,
    cap_transfer: Option<CapId>,
    reply_to: Option<MessageId>,
    sender_cap: CapId,
) -> Result<MessageId, IpcError> {
    // Validate capability
    {
        let table = CAP_TABLE.lock();
        table.validate(sender_cap, sender, PermissionBits::WRITE)
            .map_err(|_| IpcError::CapabilityDenied)?;
    }

    let msg_id = next_message_id();
    let msg = Message {
        id: msg_id,
        sender,
        recipient,
        msg_type,
        payload,
        cap_transfer,
        reply_to,
    };

    let mut mailboxes = MAILBOXES.lock();
    let mailbox = mailboxes.get_mut(&recipient).ok_or(IpcError::RecipientNotFound)?;
    mailbox.push(msg)?;

    // Wake recipient if blocked (try_lock to avoid deadlock if scheduler is locked)
    if let Some(mut sched) = crate::task::scheduler::SCHEDULER.try_lock() {
        sched.unblock(recipient);
    }

    Ok(msg_id)
}

/// Non-blocking receive.
pub fn receive(task_id: TaskId) -> Option<Message> {
    MAILBOXES.lock().get_mut(&task_id)?.pop()
}

/// Get mailbox length for a task.
pub fn mailbox_len(task_id: TaskId) -> usize {
    MAILBOXES.lock().get(&task_id).map_or(0, |mb| mb.len())
}
