use hmac::{Hmac, Mac};
use sha2::Sha256;
use spin::Mutex;
use super::rng;
use crate::println;

type HmacSha256 = Hmac<Sha256>;

static HMAC_KEY: Mutex<[u8; 32]> = Mutex::new([0u8; 32]);

pub fn init() {
    let mut key = [0u8; 32];
    rng::fill_bytes(&mut key);
    *HMAC_KEY.lock() = key;
    println!("[crypto] HMAC key generated for capability tokens");
}

pub fn generate_tag(cap_id: u64, owner_id: u64, permissions: u32) -> [u8; 32] {
    let key = HMAC_KEY.lock();
    let mut mac = HmacSha256::new_from_slice(&*key).expect("HMAC key valid");
    mac.update(&cap_id.to_le_bytes());
    mac.update(&owner_id.to_le_bytes());
    mac.update(&permissions.to_le_bytes());
    let result = mac.finalize();
    let mut tag = [0u8; 32];
    tag.copy_from_slice(&result.into_bytes());
    tag
}

/// Constant-time tag comparison.
pub fn verify_tag(cap_id: u64, owner_id: u64, permissions: u32, tag: &[u8; 32]) -> bool {
    let expected = generate_tag(cap_id, owner_id, permissions);
    let mut diff = 0u8;
    for (a, b) in expected.iter().zip(tag.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}
