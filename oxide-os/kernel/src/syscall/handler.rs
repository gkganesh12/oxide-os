use super::numbers::*;
use crate::println;
use crate::agent;
use crate::task::scheduler;
use crate::ipc;
use crate::interrupts;
use crate::timer;

/// Main syscall dispatch. Returns result as i64 (negative = error).
pub fn dispatch(number: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i64 {
    match number {
        SYS_EXIT => sys_exit(arg1 as i32),
        SYS_PRINT => sys_print(arg1 as *const u8, arg2 as usize),
        SYS_YIELD => sys_yield(),
        SYS_AGENT_LIST => sys_agent_list(),
        SYS_AGENT_STATUS => sys_agent_status(arg1),
        SYS_AGENT_KILL => sys_agent_kill(arg1),
        SYS_IPC_SEND => sys_ipc_send(arg1, arg2, arg3 as *const u8, arg4 as usize, arg5),
        SYS_IPC_RECEIVE => sys_ipc_receive(arg1),
        SYS_STORAGE_SET => sys_storage_set(arg1, arg2 as *const u8, arg3 as usize, arg4 as *const u8, arg5 as usize),
        SYS_STORAGE_GET => sys_storage_get(arg1, arg2 as *const u8, arg3 as usize),
        SYS_TIMER_SLEEP => sys_sleep(arg1),
        _ => {
            println!("[syscall] Unknown syscall: {}", number);
            -1
        }
    }
}

fn sys_exit(code: i32) -> i64 {
    println!("[syscall] exit({})", code);
    scheduler::exit_current();
}

fn sys_print(ptr: *const u8, len: usize) -> i64 {
    if ptr.is_null() || len > 4096 { return -1; }
    let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
    if let Ok(s) = core::str::from_utf8(slice) {
        crate::print!("{}", s);
        len as i64
    } else { -1 }
}

fn sys_yield() -> i64 {
    scheduler::yield_now();
    0
}

fn sys_agent_list() -> i64 {
    let registry = agent::registry::REGISTRY.lock();
    let count = registry.count();
    count as i64
}

fn sys_agent_status(agent_id: u64) -> i64 {
    let registry = agent::registry::REGISTRY.lock();
    match registry.get(agent_id) {
        Some(agent) => agent.state as i64,
        None => -1,
    }
}

fn sys_agent_kill(agent_id: u64) -> i64 {
    match agent::lifecycle::kill(agent_id) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

fn sys_ipc_send(sender: u64, recipient: u64, payload_ptr: *const u8, payload_len: usize, cap_id: u64) -> i64 {
    if payload_ptr.is_null() || payload_len > 65536 { return -1; }
    let payload = unsafe { core::slice::from_raw_parts(payload_ptr, payload_len) }.to_vec();
    match ipc::message::send(sender, recipient, ipc::MessageType::Data, payload, None, None, cap_id) {
        Ok(msg_id) => msg_id.0 as i64,
        Err(_) => -1,
    }
}

fn sys_ipc_receive(task_id: u64) -> i64 {
    match ipc::message::receive(task_id) {
        Some(_msg) => 1, // Message received (real impl would copy to user buffer)
        None => 0,       // No message
    }
}

fn sys_storage_set(agent_id: u64, key_ptr: *const u8, key_len: usize, val_ptr: *const u8, val_len: usize) -> i64 {
    if key_ptr.is_null() || val_ptr.is_null() || key_len > 256 || val_len > 65536 { return -1; }
    let key_slice = unsafe { core::slice::from_raw_parts(key_ptr, key_len) };
    let val_slice = unsafe { core::slice::from_raw_parts(val_ptr, val_len) };
    let key = match core::str::from_utf8(key_slice) { Ok(s) => s, Err(_) => return -1 };
    // For now, store without cap validation (cap validation happens at context_store level)
    crate::storage::context_store::STORE.lock().set(agent_id, key, val_slice);
    0
}

fn sys_storage_get(agent_id: u64, key_ptr: *const u8, key_len: usize) -> i64 {
    if key_ptr.is_null() || key_len > 256 { return -1; }
    let key_slice = unsafe { core::slice::from_raw_parts(key_ptr, key_len) };
    let key = match core::str::from_utf8(key_slice) { Ok(s) => s, Err(_) => return -1 };
    match crate::storage::context_store::STORE.lock().get(agent_id, key) {
        Some(data) => data.len() as i64,
        None => -1,
    }
}

fn sys_sleep(ticks: u64) -> i64 {
    let current_tick = interrupts::ticks();
    let deadline = current_tick + ticks;
    let task_id = {
        let sched = scheduler::SCHEDULER.lock();
        sched.current_task().map(|t| t.id)
    };
    if let Some(tid) = task_id {
        timer::deadline::schedule(tid, deadline);
        scheduler::block_and_yield();
    }
    0
}
