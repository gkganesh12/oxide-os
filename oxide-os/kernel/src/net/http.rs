use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::task::TaskId;
use crate::capability::{CapId, CAP_TABLE, PermissionBits};
use super::firewall;
use super::socket::SocketError;

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
    firewall::log_decision(task_id, &format!("{}:{}", host, port), &decision);
    if let firewall::FirewallDecision::Deny(_) = decision {
        return Err(HttpError::FirewallDenied);
    }

    // Resolve DNS
    let addr = super::dns::resolve(&host).ok_or(HttpError::DnsResolutionFailed)?;

    crate::println!("[http] GET {}:{}{} (task {})", host, port, path, task_id);

    // Create TCP socket and connect
    let handle = super::socket::tcp_create().ok_or(HttpError::ConnectionFailed)?;
    super::socket::tcp_connect(handle, addr, port, task_id, net_cap)
        .map_err(|_| HttpError::ConnectionFailed)?;

    // Poll network until connected (with timeout)
    let deadline = crate::interrupts::ticks() + 500;
    loop {
        super::stack::poll();
        if crate::interrupts::ticks() > deadline {
            break;
        }
        core::hint::spin_loop();
    }

    // Build and send HTTP request
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, host
    );

    match super::socket::tcp_send(handle, request.as_bytes()) {
        Ok(_) => {}
        Err(_) => {
            super::socket::tcp_close(handle);
            return Err(HttpError::SendFailed);
        }
    }

    // Receive response
    let mut response_bytes = Vec::new();
    let mut buf = [0u8; 4096];
    let recv_deadline = crate::interrupts::ticks() + 1000;
    loop {
        super::stack::poll();
        match super::socket::tcp_receive(handle, &mut buf) {
            Ok(0) => break,
            Ok(n) => response_bytes.extend_from_slice(&buf[..n]),
            Err(SocketError::NotConnected) => break,
            Err(_) => {}
        }
        if crate::interrupts::ticks() > recv_deadline {
            break;
        }
    }

    super::socket::tcp_close(handle);

    // Parse response
    if response_bytes.is_empty() {
        return Ok(HttpResponse { status_code: 0, body: Vec::new() });
    }
    parse_http_response(&response_bytes)
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
    firewall::log_decision(task_id, &format!("{}:{}", host, port), &decision);
    if let firewall::FirewallDecision::Deny(_) = decision {
        return Err(HttpError::FirewallDenied);
    }

    // Resolve DNS
    let addr = super::dns::resolve(&host).ok_or(HttpError::DnsResolutionFailed)?;

    crate::println!("[http] POST {}:{}{} ({} bytes, task {})", host, port, path, body.len(), task_id);

    // Create TCP socket and connect
    let handle = super::socket::tcp_create().ok_or(HttpError::ConnectionFailed)?;
    super::socket::tcp_connect(handle, addr, port, task_id, net_cap)
        .map_err(|_| HttpError::ConnectionFailed)?;

    // Poll network until connected (with timeout)
    let deadline = crate::interrupts::ticks() + 500;
    loop {
        super::stack::poll();
        if crate::interrupts::ticks() > deadline {
            break;
        }
        core::hint::spin_loop();
    }

    // Build and send HTTP request with body
    let request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        path, host, body.len()
    );

    match super::socket::tcp_send(handle, request.as_bytes()) {
        Ok(_) => {}
        Err(_) => {
            super::socket::tcp_close(handle);
            return Err(HttpError::SendFailed);
        }
    }

    // Send body
    match super::socket::tcp_send(handle, body) {
        Ok(_) => {}
        Err(_) => {
            super::socket::tcp_close(handle);
            return Err(HttpError::SendFailed);
        }
    }

    // Receive response
    let mut response_bytes = Vec::new();
    let mut buf = [0u8; 4096];
    let recv_deadline = crate::interrupts::ticks() + 1000;
    loop {
        super::stack::poll();
        match super::socket::tcp_receive(handle, &mut buf) {
            Ok(0) => break,
            Ok(n) => response_bytes.extend_from_slice(&buf[..n]),
            Err(SocketError::NotConnected) => break,
            Err(_) => {}
        }
        if crate::interrupts::ticks() > recv_deadline {
            break;
        }
    }

    super::socket::tcp_close(handle);

    // Parse response
    if response_bytes.is_empty() {
        return Ok(HttpResponse { status_code: 0, body: Vec::new() });
    }
    parse_http_response(&response_bytes)
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
            host_port[i+1..].parse::<u16>().unwrap_or(80),
        ),
        None => (String::from(host_port), 80),
    };
    Ok((host, port, path))
}

fn parse_http_response(data: &[u8]) -> Result<HttpResponse, HttpError> {
    let text = core::str::from_utf8(data).map_err(|_| HttpError::ParseError)?;

    // Parse status line: "HTTP/1.1 200 OK"
    let status_line = text.lines().next().ok_or(HttpError::ParseError)?;
    let status_code: u16 = status_line.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or(HttpError::ParseError)?;

    // Body is after \r\n\r\n
    let body_start = text.find("\r\n\r\n").map(|i| i + 4).unwrap_or(data.len());
    let body = data[body_start..].to_vec();

    Ok(HttpResponse { status_code, body })
}
