//! Host-broker client.
//!
//! Pure helper that wraps `net::http::post` to call the host-side broker
//! (Node.js service from PR #13). One call = one Claude inference round.
//! No global state. Capability-checked at the HTTP layer.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::capability::CapId;
use crate::net::http::{self, HttpError};
use crate::task::TaskId;

pub const DEFAULT_BROKER_ENDPOINT: &str = "http://10.0.2.2:3101";

#[derive(Debug)]
pub enum BrokerError {
    Http(HttpError),
    Non2xx(u16),
    ParseFailure,
}

#[derive(Debug)]
pub struct InferReply {
    pub text: String,
    pub tokens_in: u32,
    pub tokens_out: u32,
}

/// POST {endpoint}/v1/agent/{agent_name}/infer with body {"message": <user_message>}.
///
/// Returns the parsed reply text and token usage on success.
pub fn post_infer(
    endpoint: &str,
    agent_name: &str,
    user_message: &str,
    task_id: TaskId,
    net_cap: CapId,
) -> Result<InferReply, BrokerError> {
    let url = format!("{}/v1/agent/{}/infer", endpoint, agent_name);
    let body = build_body(user_message);

    let resp = http::post(&url, body.as_bytes(), task_id, net_cap)
        .map_err(BrokerError::Http)?;

    if !(200..300).contains(&resp.status_code) {
        return Err(BrokerError::Non2xx(resp.status_code));
    }

    let body_str = core::str::from_utf8(&resp.body).map_err(|_| BrokerError::ParseFailure)?;
    parse_reply(body_str).ok_or(BrokerError::ParseFailure)
}

/// Build `{"message":"<escaped>"}` JSON. Hand-rolled to avoid pulling in serde.
fn build_body(message: &str) -> String {
    let mut out = String::with_capacity(message.len() + 16);
    out.push_str("{\"message\":\"");
    for ch in message.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push_str("\"}");
    out
}

/// Extract `text` (string) and `usage.input_tokens` / `usage.output_tokens` (numbers)
/// from the broker's JSON response. Tolerates extra fields and arbitrary whitespace
/// but does NOT support escaped characters inside `text`.
fn parse_reply(body: &str) -> Option<InferReply> {
    let text = extract_string_field(body, "\"text\"")?;
    let tokens_in = extract_number_field(body, "\"input_tokens\"").unwrap_or(0);
    let tokens_out = extract_number_field(body, "\"output_tokens\"").unwrap_or(0);
    Some(InferReply { text, tokens_in, tokens_out })
}

fn extract_string_field(body: &str, key: &str) -> Option<String> {
    let key_pos = body.find(key)?;
    let after_key = &body[key_pos + key.len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    let rest = after_colon.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(String::from(&rest[..end]))
}

fn extract_number_field(body: &str, key: &str) -> Option<u32> {
    let key_pos = body.find(key)?;
    let after_key = &body[key_pos + key.len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    let end = after_colon.find(|c: char| !c.is_ascii_digit())?;
    after_colon[..end].parse().ok()
}
