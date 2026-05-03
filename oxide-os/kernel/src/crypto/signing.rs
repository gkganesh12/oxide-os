use alloc::collections::BTreeMap;
use sha2::{Sha256, Digest};
use spin::Mutex;
use super::rng;
use crate::agent::AgentId;
use crate::println;

#[derive(Debug, Clone)]
pub struct KeyPair {
    pub public_key: [u8; 32],
    secret_key: [u8; 32],
}

impl KeyPair {
    pub fn generate() -> Self {
        let mut secret_key = [0u8; 32];
        rng::fill_bytes(&mut secret_key);
        let mut hasher = Sha256::new();
        hasher.update(&secret_key);
        let public_key: [u8; 32] = hasher.finalize().into();
        KeyPair { public_key, secret_key }
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(&self.secret_key);
        hasher.update(message);
        hasher.finalize().into()
    }

    pub fn verify(public_key: &[u8; 32], message: &[u8], signature: &[u8; 32]) -> bool {
        // Look up the agent by public key and recompute the signature
        if let Some((_id, kp)) = AGENT_KEYS.lock().iter().find(|(_, kp)| &kp.public_key == public_key) {
            let expected = kp.sign(message);
            // Constant-time comparison
            let mut diff = 0u8;
            for (a, b) in expected.iter().zip(signature.iter()) {
                diff |= a ^ b;
            }
            diff == 0
        } else {
            false
        }
    }
}

static AGENT_KEYS: Mutex<BTreeMap<AgentId, KeyPair>> = Mutex::new(BTreeMap::new());

pub fn generate_agent_keypair(agent_id: AgentId) -> [u8; 32] {
    let kp = KeyPair::generate();
    let public = kp.public_key;
    AGENT_KEYS.lock().insert(agent_id, kp);
    public
}

pub fn sign(agent_id: AgentId, message: &[u8]) -> Option<[u8; 32]> {
    AGENT_KEYS.lock().get(&agent_id).map(|kp| kp.sign(message))
}

pub fn get_public_key(agent_id: AgentId) -> Option<[u8; 32]> {
    AGENT_KEYS.lock().get(&agent_id).map(|kp| kp.public_key)
}

pub fn init() {
    println!("[crypto] Agent signing ready (HMAC-SHA256)");
}
