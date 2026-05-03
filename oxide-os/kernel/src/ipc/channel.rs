use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use core::sync::atomic::{AtomicU64, Ordering};
use super::{IpcError, MessageType, message};
use crate::task::TaskId;
use crate::capability::{CapId, CAP_TABLE, PermissionBits};
use crate::println;

pub type ChannelId = u64;

pub struct Channel {
    pub id: ChannelId,
    pub name: String,
    pub subscribers: Vec<(TaskId, CapId)>,
}

static NEXT_CHANNEL_ID: AtomicU64 = AtomicU64::new(1);
static CHANNELS: Mutex<BTreeMap<String, Channel>> = Mutex::new(BTreeMap::new());

pub fn create(name: String) -> ChannelId {
    let id = NEXT_CHANNEL_ID.fetch_add(1, Ordering::Relaxed);
    let channel = Channel { id, name: name.clone(), subscribers: Vec::new() };
    println!("[channel] Created '{}' (id: {})", name, id);
    CHANNELS.lock().insert(name, channel);
    id
}

pub fn subscribe(channel_name: &str, task_id: TaskId, cap_id: CapId) -> Result<(), IpcError> {
    {
        let table = CAP_TABLE.lock();
        table.validate(cap_id, task_id, PermissionBits::SUBSCRIBE)
            .map_err(|_| IpcError::CapabilityDenied)?;
    }

    let mut channels = CHANNELS.lock();
    let channel = channels.get_mut(channel_name).ok_or(IpcError::RecipientNotFound)?;
    if !channel.subscribers.iter().any(|(t, _)| *t == task_id) {
        channel.subscribers.push((task_id, cap_id));
    }
    Ok(())
}

pub fn unsubscribe(channel_name: &str, task_id: TaskId) -> Result<(), IpcError> {
    let mut channels = CHANNELS.lock();
    let channel = channels.get_mut(channel_name).ok_or(IpcError::RecipientNotFound)?;
    channel.subscribers.retain(|(t, _)| *t != task_id);
    Ok(())
}

/// Publish to all subscribers. Requires PUBLISH capability.
pub fn publish(
    channel_name: &str,
    sender: TaskId,
    payload: Vec<u8>,
    publish_cap: CapId,
) -> Result<usize, IpcError> {
    {
        let table = CAP_TABLE.lock();
        table.validate(publish_cap, sender, PermissionBits::PUBLISH)
            .map_err(|_| IpcError::CapabilityDenied)?;
    }

    let subscribers: Vec<TaskId> = {
        let channels = CHANNELS.lock();
        let channel = channels.get(channel_name).ok_or(IpcError::RecipientNotFound)?;
        channel.subscribers.iter().map(|(t, _)| *t).filter(|&t| t != sender).collect()
    };

    let count = subscribers.len();
    let mut mailboxes = message::MAILBOXES.lock();
    for &sub_task in &subscribers {
        if let Some(mailbox) = mailboxes.get_mut(&sub_task) {
            let msg = message::Message {
                id: super::MessageId(0),
                sender,
                recipient: sub_task,
                msg_type: MessageType::Data,
                payload: payload.clone(),
                cap_transfer: None,
                reply_to: None,
            };
            let _ = mailbox.push(msg); // Best-effort delivery
        }
    }

    Ok(count)
}
