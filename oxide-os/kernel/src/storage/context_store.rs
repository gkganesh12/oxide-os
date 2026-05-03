use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;
use super::oxide_fs::FS;
use crate::agent::AgentId;
use crate::capability::{CapId, CAP_TABLE, PermissionBits};
use crate::println;

pub struct ContextStore {
    cache: BTreeMap<AgentId, BTreeMap<String, Vec<u8>>>,
}

impl ContextStore {
    pub const fn new() -> Self {
        ContextStore { cache: BTreeMap::new() }
    }

    pub fn set(&mut self, agent_id: AgentId, key: &str, value: &[u8]) {
        let store = self.cache.entry(agent_id).or_insert_with(BTreeMap::new);
        store.insert(String::from(key), value.to_vec());
        let path = alloc::format!("/agents/{}/{}", agent_id, key);
        FS.lock().write_file(&path, value);
    }

    pub fn get(&self, agent_id: AgentId, key: &str) -> Option<Vec<u8>> {
        if let Some(store) = self.cache.get(&agent_id) {
            if let Some(val) = store.get(key) { return Some(val.clone()); }
        }
        let path = alloc::format!("/agents/{}/{}", agent_id, key);
        FS.lock().read_file(&path).map(|d| d.to_vec())
    }

    pub fn delete(&mut self, agent_id: AgentId, key: &str) -> bool {
        let from_cache = self.cache.get_mut(&agent_id).map(|s| s.remove(key).is_some()).unwrap_or(false);
        let path = alloc::format!("/agents/{}/{}", agent_id, key);
        let from_fs = FS.lock().delete_file(&path);
        from_cache || from_fs
    }

    pub fn keys(&self, agent_id: AgentId) -> Vec<String> {
        let prefix = alloc::format!("/agents/{}/", agent_id);
        FS.lock().list(&prefix).iter().map(|e| {
            e.path.strip_prefix(&prefix as &str).unwrap_or(&e.path).to_string()
        }).collect()
    }

    pub fn clear_agent(&mut self, agent_id: AgentId) {
        self.cache.remove(&agent_id);
        let prefix = alloc::format!("/agents/{}/", agent_id);
        let mut fs = FS.lock();
        let paths: Vec<String> = fs.list(&prefix).iter().map(|e| e.path.clone()).collect();
        for path in paths { fs.delete_file(&path); }
    }
}

pub static STORE: Mutex<ContextStore> = Mutex::new(ContextStore::new());

/// Capability-gated set.
pub fn set(agent_id: AgentId, key: &str, value: &[u8], cap_id: CapId) -> Result<(), &'static str> {
    CAP_TABLE.lock().validate(cap_id, agent_id, PermissionBits::WRITE)
        .map_err(|_| "insufficient storage capability")?;
    STORE.lock().set(agent_id, key, value);
    Ok(())
}

/// Capability-gated get.
pub fn get(agent_id: AgentId, key: &str, cap_id: CapId) -> Result<Option<Vec<u8>>, &'static str> {
    CAP_TABLE.lock().validate(cap_id, agent_id, PermissionBits::READ)
        .map_err(|_| "insufficient storage capability")?;
    Ok(STORE.lock().get(agent_id, key))
}

pub fn init() {
    println!("[storage] Agent context store initialized");
}
