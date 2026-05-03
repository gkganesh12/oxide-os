use alloc::string::String;
use alloc::vec::Vec;
use crate::task::TaskId;
use crate::capability::CapId;

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
}

pub fn get(url: &str, task_id: TaskId, _net_cap: CapId) -> Result<HttpResponse, HttpError> {
    let (host, port, path) = parse_url(url)?;
    // For now: log the request. Real implementation connects via socket API.
    crate::println!("[http] GET {}:{}{} (task {})", host, port, path, task_id);
    // Return a placeholder — real networking requires virtio-net packet handling
    Ok(HttpResponse { status_code: 200, body: b"{}".to_vec() })
}

pub fn post(url: &str, body: &[u8], task_id: TaskId, _net_cap: CapId) -> Result<HttpResponse, HttpError> {
    let (host, port, path) = parse_url(url)?;
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
