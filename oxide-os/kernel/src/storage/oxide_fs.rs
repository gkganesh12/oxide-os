use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use crate::println;

pub type BlobId = u64;

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub blob_id: BlobId,
    pub size: u64,
    pub created_tick: u64,
    pub modified_tick: u64,
    pub deleted: bool,
}

pub struct OxideFs {
    index: BTreeMap<String, FileEntry>,
    blobs: BTreeMap<BlobId, Vec<u8>>,
    /// Hash → BlobId for O(1) dedup lookups (FNV-style hash of content)
    content_hashes: BTreeMap<u64, BlobId>,
    next_blob_id: BlobId,
    total_bytes: u64,
}

impl OxideFs {
    pub const fn new() -> Self {
        OxideFs {
            index: BTreeMap::new(), blobs: BTreeMap::new(),
            content_hashes: BTreeMap::new(), next_blob_id: 1, total_bytes: 0,
        }
    }

    /// Simple FNV-1a hash for content dedup. Not cryptographic — just fast.
    fn content_hash(data: &[u8]) -> u64 {
        let mut hash: u64 = 0xcbf29ce484222325;
        for &byte in data {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }

    pub fn write_file(&mut self, path: &str, data: &[u8]) -> BlobId {
        let hash = Self::content_hash(data);

        // O(1) dedup via hash lookup
        if let Some(&existing_id) = self.content_hashes.get(&hash) {
            // Verify actual content matches (hash collision protection)
            if let Some(blob) = self.blobs.get(&existing_id) {
                if blob.as_slice() == data {
                    let tick = crate::interrupts::ticks();
                    self.index.insert(String::from(path), FileEntry {
                        path: String::from(path), blob_id: existing_id, size: data.len() as u64,
                        created_tick: tick, modified_tick: tick, deleted: false,
                    });
                    return existing_id;
                }
            }
        }

        let blob_id = self.next_blob_id;
        self.next_blob_id += 1;
        self.blobs.insert(blob_id, data.to_vec());
        self.content_hashes.insert(hash, blob_id);
        self.total_bytes += data.len() as u64;

        let tick = crate::interrupts::ticks();
        self.index.insert(String::from(path), FileEntry {
            path: String::from(path), blob_id, size: data.len() as u64,
            created_tick: tick, modified_tick: tick, deleted: false,
        });
        blob_id
    }

    pub fn read_file(&self, path: &str) -> Option<&[u8]> {
        let entry = self.index.get(path)?;
        if entry.deleted { return None; }
        self.blobs.get(&entry.blob_id).map(|v| v.as_slice())
    }

    pub fn delete_file(&mut self, path: &str) -> bool {
        if let Some(entry) = self.index.get_mut(path) {
            entry.deleted = true;
            true
        } else { false }
    }

    pub fn list(&self, prefix: &str) -> Vec<&FileEntry> {
        self.index.iter()
            .filter(|(p, e)| p.starts_with(prefix) && !e.deleted)
            .map(|(_, e)| e).collect()
    }

    pub fn stats(&self) -> (usize, usize, u64) {
        let files = self.index.values().filter(|e| !e.deleted).count();
        (files, self.blobs.len(), self.total_bytes)
    }
}

pub static FS: Mutex<OxideFs> = Mutex::new(OxideFs::new());

pub fn init() {
    println!("[storage] OxideFS initialized (log-structured, content-addressable)");
}
