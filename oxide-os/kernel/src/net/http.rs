use alloc::string::String;
use alloc::vec::Vec;
use crate::task::TaskId;
use crate::capability::{CapId, CAP_TABLE, PermissionBits};
use super::firewall;

#[derive(Debug)]
pub struct HttpResponse {
    pub status_code: u16,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub enum HttpError {
    DnsResolutionFailed,
    ConnectionFailed,
    SendFailed,
    ReceiveFailed,
    ParseError,
    CapabilityDenied,
    FirewallDenied,
}

pub fn get(url: &str, task_id: TaskId, net_cap: CapId) -> Result<HttpResponse, HttpError> {
    let (host, port, path) = parse_url(url)?;

    // Validate capability
    {
        let table = CAP_TABLE.lock();
        table.validate(net_cap, task_id, PermissionBits::CONNECT)
            .map_err(|_| HttpError::CapabilityDenied)?;
    }

    // Check firewall
    let decision = firewall::check_outbound(task_id, net_cap, &host, port);
    firewall::log_decision(task_id, &alloc::format!("{}:{}", host, port), &decision);
    if let firewall::FirewallDecision::Deny(_) = decision {
        return Err(HttpError::FirewallDenied);
    }

    crate::println!("[http] GET {}:{}{} (task {})", host, port, path, task_id);
    // Placeholder — real networking requires virtio-net packet handling
    Ok(HttpResponse { status_code: 200, body: b"{}".to_vec() })
}

pub fn post(url: &str, body: &[u8], task_id: TaskId, net_cap: CapId) -> Result<HttpResponse, HttpError> {
    let (host, port, path) = parse_url(url)?;

    // Validate capability
    {
        let table = CAP_TABLE.lock();
        table.validate(net_cap, task_id, PermissionBits::CONNECT)
            .map_err(|_| HttpError::CapabilityDenied)?;
    }

    // Check firewall
    let decision = firewall::check_outbound(task_id, net_cap, &host, port);
    firewall::log_decision(task_id, &alloc::format!("{}:{}", host, port), &decision);
    if let firewall::FirewallDecision::Deny(_) = decision {
        return Err(HttpError::FirewallDenied);
    }

    crate::println!("[http] POST {}:{}{} ({} bytes, task {})", host, port, path, body.len(), task_id);
    Ok(HttpResponse { status_code: 200, body: b"{}".to_vec() })
}

fn parse_url(url: &str) -> Result<(String, u16, String), HttpError> {
    let url = url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let (host_port, path) = match url.find('/') {
        Some(i) => (&url[..i], String::from(&url[i..])),
        None => (url, String::from("/")),
    };
    let (host, port) = match host_port.find(':') {
        Some(i) => (
            String::from(&host_port[..i]),
            host_port[i+1..].parse::<u16>().unwrap_or(443),
        ),
        None => (String::from(host_port), 443),
    };
    Ok((host, port, path))
}
