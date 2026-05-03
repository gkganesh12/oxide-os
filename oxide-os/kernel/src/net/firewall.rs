use crate::capability::{CapId, CAP_TABLE, PermissionBits, ResourceRef};
use crate::task::TaskId;
use crate::println;

#[derive(Debug)]
pub enum FirewallDecision {
    Allow,
    Deny(&'static str),
}

pub fn check_outbound(task_id: TaskId, cap_id: CapId, target_host: &str, target_port: u16) -> FirewallDecision {
    let table = CAP_TABLE.lock();
    match table.validate(cap_id, task_id, PermissionBits::CONNECT) {
        Err(_) => FirewallDecision::Deny("no valid network capability"),
        Ok(cap) => {
            match &cap.resource {
                ResourceRef::Network { host, port } => {
                    if host != "*" && host != target_host {
                        return FirewallDecision::Deny("host not allowed");
                    }
                    if *port != 0 && *port != target_port {
                        return FirewallDecision::Deny("port not allowed");
                    }
                    FirewallDecision::Allow
                }
                _ => FirewallDecision::Deny("not a network capability"),
            }
        }
    }
}

pub fn log_decision(task_id: TaskId, target: &str, decision: &FirewallDecision) {
    match decision {
        FirewallDecision::Allow => println!("[firewall] ALLOW task {} -> {}", task_id, target),
        FirewallDecision::Deny(reason) => println!("[firewall] DENY task {} -> {} ({})", task_id, target, reason),
    }
}
