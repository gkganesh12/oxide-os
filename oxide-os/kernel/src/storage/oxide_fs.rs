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

    /// Flush filesystem state to disk via block cache.
    pub fn sync_to_disk(&self) {
        use super::block_cache::CACHE;
        let mut cache = CACHE.lock();

        // Write superblock (block 0)
        let mut sb = [0u8; 512];
        sb[0..8].copy_from_slice(b"OXIDEFS\0");
        let blob_count = self.blobs.len() as u32;
        let index_count = self.index.values().filter(|e| !e.deleted).count() as u32;
        sb[8..12].copy_from_slice(&1u32.to_le_bytes()); // version
        sb[12..16].copy_from_slice(&blob_count.to_le_bytes());
        sb[16..20].copy_from_slice(&index_count.to_le_bytes());
        sb[20..28].copy_from_slice(&self.next_blob_id.to_le_bytes());
        sb[28..36].copy_from_slice(&self.total_bytes.to_le_bytes());
        let _ = cache.write(0, sb);

        // Write blobs starting at block 1
        let mut block_num = 1u64;
        for (&blob_id, data) in &self.blobs {
            // Header: blob_id (8) + size (4) = 12 bytes
            let mut block = [0u8; 512];
            block[0..8].copy_from_slice(&blob_id.to_le_bytes());
            block[8..12].copy_from_slice(&(data.len() as u32).to_le_bytes());
            // Copy data (may need multiple blocks)
            let first_chunk = data.len().min(500); // 512 - 12 header
            block[12..12+first_chunk].copy_from_slice(&data[..first_chunk]);
            let _ = cache.write(block_num, block);
            block_num += 1;

            // Continuation blocks for large blobs
            let mut offset = first_chunk;
            while offset < data.len() {
                let mut cont = [0u8; 512];
                let chunk = (data.len() - offset).min(512);
                cont[..chunk].copy_from_slice(&data[offset..offset+chunk]);
                let _ = cache.write(block_num, cont);
                block_num += 1;
                offset += chunk;
            }
        }

        // Write index entries
        for (path, entry) in &self.index {
            if entry.deleted { continue; }
            let mut block = [0u8; 512];
            let path_bytes = path.as_bytes();
            let path_len = path_bytes.len().min(255) as u16;
            block[0..2].copy_from_slice(&path_len.to_le_bytes());
            block[2..2+path_len as usize].copy_from_slice(&path_bytes[..path_len as usize]);
            let off = 2 + path_len as usize;
            block[off..off+8].copy_from_slice(&entry.blob_id.to_le_bytes());
            block[off+8..off+16].copy_from_slice(&entry.size.to_le_bytes());
            block[off+16..off+24].copy_from_slice(&entry.created_tick.to_le_bytes());
            block[off+24..off+32].copy_from_slice(&entry.modified_tick.to_le_bytes());
            let _ = cache.write(block_num, block);
            block_num += 1;
        }

        // Flush cache to disk
        let _ = cache.flush();
        crate::println!("[oxidefs] Synced to disk: {} blobs, {} entries, {} blocks", blob_count, index_count, block_num);
    }

    /// Load filesystem state from disk.
    pub fn load_from_disk(&mut self) -> bool {
        use super::block_cache::CACHE;
        let mut cache = CACHE.lock();

        // Read superblock
        let sb = match cache.read(0) {
            Ok(data) => data,
            Err(_) => return false,
        };

        if &sb[0..8] != b"OXIDEFS\0" {
            return false; // No valid filesystem
        }

        let blob_count = u32::from_le_bytes(sb[12..16].try_into().unwrap()) as usize;
        let index_count = u32::from_le_bytes(sb[16..20].try_into().unwrap()) as usize;
        self.next_blob_id = u64::from_le_bytes(sb[20..28].try_into().unwrap());
        self.total_bytes = u64::from_le_bytes(sb[28..36].try_into().unwrap());

        crate::println!("[oxidefs] Loading from disk: {} blobs, {} entries", blob_count, index_count);

        // Read blobs
        let mut block_num = 1u64;
        for _ in 0..blob_count {
            let block = cache.read(block_num).unwrap_or([0; 512]);
            let blob_id = u64::from_le_bytes(block[0..8].try_into().unwrap());
            let size = u32::from_le_bytes(block[8..12].try_into().unwrap()) as usize;
            block_num += 1;

            let mut data = Vec::with_capacity(size);
            let first_chunk = size.min(500);
            data.extend_from_slice(&block[12..12+first_chunk]);

            let mut remaining = size - first_chunk;
            while remaining > 0 {
                let cont = cache.read(block_num).unwrap_or([0; 512]);
                let chunk = remaining.min(512);
                data.extend_from_slice(&cont[..chunk]);
                remaining -= chunk;
                block_num += 1;
            }

            let hash = Self::content_hash(&data);
            self.content_hashes.insert(hash, blob_id);
            self.blobs.insert(blob_id, data);
        }

        // Read index entries
        for _ in 0..index_count {
            let block = cache.read(block_num).unwrap_or([0; 512]);
            let path_len = u16::from_le_bytes(block[0..2].try_into().unwrap()) as usize;
            let path = String::from(core::str::from_utf8(&block[2..2+path_len]).unwrap_or(""));
            let off = 2 + path_len;
            let blob_id = u64::from_le_bytes(block[off..off+8].try_into().unwrap());
            let size = u64::from_le_bytes(block[off+8..off+16].try_into().unwrap());
            let created = u64::from_le_bytes(block[off+16..off+24].try_into().unwrap());
            let modified = u64::from_le_bytes(block[off+24..off+32].try_into().unwrap());

            self.index.insert(path.clone(), FileEntry {
                path, blob_id, size, created_tick: created, modified_tick: modified, deleted: false,
            });
            block_num += 1;
        }

        true
    }
}

pub static FS: Mutex<OxideFs> = Mutex::new(OxideFs::new());

pub fn init() {
    let loaded = FS.lock().load_from_disk();
    if loaded {
        let (files, blobs, bytes) = FS.lock().stats();
        println!("[storage] OxideFS loaded from disk: {} files, {} blobs, {} bytes", files, blobs, bytes);
    } else {
        println!("[storage] OxideFS initialized (fresh, content-addressable)");
    }
}
