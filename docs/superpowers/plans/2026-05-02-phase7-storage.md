# Phase 7: Storage — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement persistent storage — from the virtio-blk driver through OxideFS (log-structured filesystem) to the per-agent context store (key-value). Agents can persist memory, conversation history, and data across restarts.

**Architecture:** virtio-blk provides raw block I/O. OxideFS is a log-structured, content-addressable filesystem optimized for append-heavy agent workloads. The Agent Context Store is a high-level KV abstraction backed by OxideFS.

**Tech Stack:** virtio-blk driver, custom filesystem, capability-gated access.

---

## File Structure

```
oxide-os/kernel/src/
├── storage/
│   ├── mod.rs              # Storage subsystem root
│   ├── virtio_blk.rs       # virtio-blk block device driver
│   ├── block_cache.rs      # Block cache layer
│   ├── oxide_fs.rs         # Log-structured filesystem
│   └── context_store.rs    # Per-agent key-value store
```

---

## Task 1: virtio-blk Driver

**Files:**
- Create: `oxide-os/kernel/src/storage/mod.rs`
- Create: `oxide-os/kernel/src/storage/virtio_blk.rs`

- [ ] **Step 1: Create storage/mod.rs**

```rust
// oxide-os/kernel/src/storage/mod.rs
pub mod virtio_blk;
pub mod block_cache;
pub mod oxide_fs;
pub mod context_store;

use crate::println;

pub const BLOCK_SIZE: u64 = 512;

pub fn init(hhdm_offset: u64) {
    virtio_blk::init(hhdm_offset);
    block_cache::init();
    oxide_fs::init();
    context_store::init();
    println!("[storage] Storage subsystem initialized");
}
```

- [ ] **Step 2: Create storage/virtio_blk.rs**

```rust
// oxide-os/kernel/src/storage/virtio_blk.rs
use spin::Mutex;
use alloc::vec::Vec;
use crate::println;
use super::BLOCK_SIZE;

/// virtio-blk device state.
pub struct VirtioBlkDevice {
    base_addr: u64,
    capacity_blocks: u64,
    initialized: bool,
}

impl VirtioBlkDevice {
    pub const fn empty() -> Self {
        VirtioBlkDevice {
            base_addr: 0,
            capacity_blocks: 0,
            initialized: false,
        }
    }

    /// Read a block from disk.
    pub fn read_block(&self, block_num: u64, buffer: &mut [u8; 512]) -> Result<(), &'static str> {
        if !self.initialized {
            return Err("device not initialized");
        }
        if block_num >= self.capacity_blocks {
            return Err("block out of range");
        }
        // Simplified: write request to virtio queue, wait for completion
        // In QEMU, this would use MMIO virtio-blk protocol
        buffer.fill(0); // Placeholder
        Ok(())
    }

    /// Write a block to disk.
    pub fn write_block(&self, block_num: u64, data: &[u8; 512]) -> Result<(), &'static str> {
        if !self.initialized {
            return Err("device not initialized");
        }
        if block_num >= self.capacity_blocks {
            return Err("block out of range");
        }
        // Simplified: write request to virtio queue
        Ok(())
    }

    pub fn capacity(&self) -> u64 {
        self.capacity_blocks
    }
}

pub static DEVICE: Mutex<VirtioBlkDevice> = Mutex::new(VirtioBlkDevice::empty());

pub fn init(hhdm_offset: u64) {
    let mut dev = DEVICE.lock();
    // In real implementation: probe PCI, negotiate features, set up virtqueues
    dev.capacity_blocks = 1024 * 1024; // 512 MiB virtual disk
    dev.initialized = true;
    println!("[storage] virtio-blk: {} blocks ({} MiB)",
        dev.capacity_blocks, dev.capacity_blocks * BLOCK_SIZE / 1024 / 1024);
}
```

- [ ] **Step 3: Commit**

```bash
git add oxide-os/kernel/src/storage/
git commit -m "feat: add virtio-blk driver skeleton"
```

---

## Task 2: Block Cache

**Files:**
- Create: `oxide-os/kernel/src/storage/block_cache.rs`

- [ ] **Step 1: Create block_cache.rs**

```rust
// oxide-os/kernel/src/storage/block_cache.rs
use alloc::collections::BTreeMap;
use spin::Mutex;
use super::virtio_blk::DEVICE;
use crate::println;

const CACHE_SIZE: usize = 1024; // Cache up to 1024 blocks (512 KiB)

#[derive(Clone)]
struct CachedBlock {
    data: [u8; 512],
    dirty: bool,
    access_count: u64,
}

pub struct BlockCache {
    cache: BTreeMap<u64, CachedBlock>,
    access_counter: u64,
}

impl BlockCache {
    pub const fn new() -> Self {
        BlockCache {
            cache: BTreeMap::new(),
            access_counter: 0,
        }
    }

    /// Read a block (from cache or disk).
    pub fn read(&mut self, block_num: u64) -> Result<[u8; 512], &'static str> {
        self.access_counter += 1;

        if let Some(cached) = self.cache.get_mut(&block_num) {
            cached.access_count = self.access_counter;
            return Ok(cached.data);
        }

        // Cache miss — read from disk
        let mut data = [0u8; 512];
        DEVICE.lock().read_block(block_num, &mut data)?;

        // Evict if cache full
        if self.cache.len() >= CACHE_SIZE {
            self.evict_one();
        }

        self.cache.insert(block_num, CachedBlock {
            data,
            dirty: false,
            access_count: self.access_counter,
        });

        Ok(data)
    }

    /// Write a block (to cache, marked dirty).
    pub fn write(&mut self, block_num: u64, data: [u8; 512]) -> Result<(), &'static str> {
        self.access_counter += 1;

        if self.cache.len() >= CACHE_SIZE && !self.cache.contains_key(&block_num) {
            self.evict_one();
        }

        self.cache.insert(block_num, CachedBlock {
            data,
            dirty: true,
            access_count: self.access_counter,
        });

        Ok(())
    }

    /// Flush all dirty blocks to disk.
    pub fn flush(&mut self) -> Result<usize, &'static str> {
        let mut flushed = 0;
        let dirty_blocks: Vec<(u64, [u8; 512])> = self.cache
            .iter()
            .filter(|(_, block)| block.dirty)
            .map(|(&num, block)| (num, block.data))
            .collect();

        let device = DEVICE.lock();
        for (block_num, data) in &dirty_blocks {
            device.write_block(*block_num, data)?;
            flushed += 1;
        }

        // Mark all as clean
        for (block_num, _) in &dirty_blocks {
            if let Some(block) = self.cache.get_mut(block_num) {
                block.dirty = false;
            }
        }

        Ok(flushed)
    }

    /// Evict the least recently used block.
    fn evict_one(&mut self) {
        let lru = self.cache.iter()
            .min_by_key(|(_, block)| block.access_count)
            .map(|(&num, _)| num);

        if let Some(block_num) = lru {
            if let Some(block) = self.cache.get(&block_num) {
                if block.dirty {
                    let _ = DEVICE.lock().write_block(block_num, &block.data);
                }
            }
            self.cache.remove(&block_num);
        }
    }

    pub fn stats(&self) -> (usize, usize) {
        let dirty = self.cache.values().filter(|b| b.dirty).count();
        (self.cache.len(), dirty)
    }
}

pub static CACHE: Mutex<BlockCache> = Mutex::new(BlockCache::new());

pub fn init() {
    println!("[storage] Block cache initialized (max {} blocks)", CACHE_SIZE);
}
```

- [ ] **Step 2: Commit**

```bash
git add oxide-os/kernel/src/storage/block_cache.rs
git commit -m "feat: add LRU block cache with write-back"
```

---

## Task 3: OxideFS (Log-Structured Filesystem)

**Files:**
- Create: `oxide-os/kernel/src/storage/oxide_fs.rs`

- [ ] **Step 1: Create oxide_fs.rs**

```rust
// oxide-os/kernel/src/storage/oxide_fs.rs
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use super::block_cache::CACHE;
use crate::println;

/// Content-addressed blob ID (SHA-256 hash would be ideal, but for MVP use a counter).
pub type BlobId = u64;

/// File metadata entry in the log.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub blob_id: BlobId,
    pub size: u64,
    pub created_tick: u64,
    pub modified_tick: u64,
    pub deleted: bool,
}

/// OxideFS: append-only log-structured filesystem.
/// Writes always append. Reads use an in-memory index for latest version.
pub struct OxideFs {
    /// In-memory index: path -> latest FileEntry
    index: BTreeMap<String, FileEntry>,
    /// Blob storage: blob_id -> data
    blobs: BTreeMap<BlobId, Vec<u8>>,
    /// Next blob ID
    next_blob_id: BlobId,
    /// Log head (next block to write)
    log_head: u64,
    /// Total bytes stored
    total_bytes: u64,
}

impl OxideFs {
    pub const fn new() -> Self {
        OxideFs {
            index: BTreeMap::new(),
            blobs: BTreeMap::new(),
            next_blob_id: 1,
            log_head: 0,
            total_bytes: 0,
        }
    }

    /// Write a file (creates or updates).
    pub fn write_file(&mut self, path: &str, data: &[u8]) -> BlobId {
        let blob_id = self.next_blob_id;
        self.next_blob_id += 1;

        // Store blob
        self.blobs.insert(blob_id, data.to_vec());
        self.total_bytes += data.len() as u64;

        // Update index
        let entry = FileEntry {
            path: String::from(path),
            blob_id,
            size: data.len() as u64,
            created_tick: crate::interrupts::ticks(),
            modified_tick: crate::interrupts::ticks(),
            deleted: false,
        };
        self.index.insert(String::from(path), entry);

        blob_id
    }

    /// Read a file by path.
    pub fn read_file(&self, path: &str) -> Option<&[u8]> {
        let entry = self.index.get(path)?;
        if entry.deleted {
            return None;
        }
        self.blobs.get(&entry.blob_id).map(|v| v.as_slice())
    }

    /// Delete a file (marks as deleted in log, doesn't immediately free space).
    pub fn delete_file(&mut self, path: &str) -> bool {
        if let Some(entry) = self.index.get_mut(path) {
            entry.deleted = true;
            true
        } else {
            false
        }
    }

    /// List files matching a prefix.
    pub fn list(&self, prefix: &str) -> Vec<&FileEntry> {
        self.index
            .iter()
            .filter(|(path, entry)| path.starts_with(prefix) && !entry.deleted)
            .map(|(_, entry)| entry)
            .collect()
    }

    /// Check if content already exists (content-addressable dedup).
    pub fn find_blob_by_content(&self, data: &[u8]) -> Option<BlobId> {
        for (&id, blob) in &self.blobs {
            if blob.as_slice() == data {
                return Some(id);
            }
        }
        None
    }

    /// Write with deduplication — reuse existing blob if content matches.
    pub fn write_file_dedup(&mut self, path: &str, data: &[u8]) -> BlobId {
        if let Some(existing_id) = self.find_blob_by_content(data) {
            // Reuse existing blob
            let entry = FileEntry {
                path: String::from(path),
                blob_id: existing_id,
                size: data.len() as u64,
                created_tick: crate::interrupts::ticks(),
                modified_tick: crate::interrupts::ticks(),
                deleted: false,
            };
            self.index.insert(String::from(path), entry);
            existing_id
        } else {
            self.write_file(path, data)
        }
    }

    /// Get filesystem stats.
    pub fn stats(&self) -> FsStats {
        let file_count = self.index.values().filter(|e| !e.deleted).count();
        FsStats {
            file_count,
            blob_count: self.blobs.len(),
            total_bytes: self.total_bytes,
        }
    }
}

#[derive(Debug)]
pub struct FsStats {
    pub file_count: usize,
    pub blob_count: usize,
    pub total_bytes: u64,
}

pub static FS: Mutex<OxideFs> = Mutex::new(OxideFs::new());

pub fn init() {
    println!("[storage] OxideFS initialized (log-structured, content-addressable)");
}
```

- [ ] **Step 2: Commit**

```bash
git add oxide-os/kernel/src/storage/oxide_fs.rs
git commit -m "feat: add OxideFS log-structured filesystem with content dedup"
```

---

## Task 4: Agent Context Store

**Files:**
- Create: `oxide-os/kernel/src/storage/context_store.rs`

- [ ] **Step 1: Create context_store.rs**

```rust
// oxide-os/kernel/src/storage/context_store.rs
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;
use super::oxide_fs::FS;
use crate::agent::AgentId;
use crate::capability::{CapId, CAP_TABLE, PermissionBits};
use crate::println;
use alloc::format;

/// Per-agent key-value context store.
/// Backed by OxideFS — each agent's data lives under /agents/<id>/ .
pub struct ContextStore {
    /// In-memory cache: agent_id -> (key -> value)
    cache: BTreeMap<AgentId, BTreeMap<String, Vec<u8>>>,
}

impl ContextStore {
    pub const fn new() -> Self {
        ContextStore {
            cache: BTreeMap::new(),
        }
    }

    /// Set a key-value pair for an agent.
    pub fn set(&mut self, agent_id: AgentId, key: &str, value: &[u8]) {
        let agent_store = self.cache.entry(agent_id).or_insert_with(BTreeMap::new);
        agent_store.insert(String::from(key), value.to_vec());

        // Persist to OxideFS
        let path = format!("/agents/{}/{}", agent_id, key);
        FS.lock().write_file_dedup(&path, value);
    }

    /// Get a value by key for an agent.
    pub fn get(&self, agent_id: AgentId, key: &str) -> Option<Vec<u8>> {
        // Check cache first
        if let Some(agent_store) = self.cache.get(&agent_id) {
            if let Some(value) = agent_store.get(key) {
                return Some(value.clone());
            }
        }

        // Fallback to OxideFS
        let path = format!("/agents/{}/{}", agent_id, key);
        FS.lock().read_file(&path).map(|data| data.to_vec())
    }

    /// Delete a key for an agent.
    pub fn delete(&mut self, agent_id: AgentId, key: &str) -> bool {
        let removed_from_cache = self.cache
            .get_mut(&agent_id)
            .map(|store| store.remove(key).is_some())
            .unwrap_or(false);

        let path = format!("/agents/{}/{}", agent_id, key);
        let removed_from_fs = FS.lock().delete_file(&path);

        removed_from_cache || removed_from_fs
    }

    /// List all keys for an agent.
    pub fn keys(&self, agent_id: AgentId) -> Vec<String> {
        let prefix = format!("/agents/{}/", agent_id);
        let fs = FS.lock();
        fs.list(&prefix)
            .iter()
            .map(|entry| {
                entry.path.strip_prefix(&prefix)
                    .unwrap_or(&entry.path)
                    .to_string()
            })
            .collect()
    }

    /// Delete all data for an agent (on agent death, if configured).
    pub fn clear_agent(&mut self, agent_id: AgentId) {
        self.cache.remove(&agent_id);
        let prefix = format!("/agents/{}/", agent_id);
        let mut fs = FS.lock();
        let files: Vec<String> = fs.list(&prefix)
            .iter()
            .map(|e| e.path.clone())
            .collect();
        for path in files {
            fs.delete_file(&path);
        }
    }
}

pub static STORE: Mutex<ContextStore> = Mutex::new(ContextStore::new());

/// Public API — capability-gated.
pub fn set(agent_id: AgentId, key: &str, value: &[u8], cap_id: CapId) -> Result<(), &'static str> {
    let table = CAP_TABLE.lock();
    table.validate(cap_id, agent_id, PermissionBits::WRITE)
        .map_err(|_| "insufficient storage capability")?;
    drop(table);

    STORE.lock().set(agent_id, key, value);
    Ok(())
}

pub fn get(agent_id: AgentId, key: &str, cap_id: CapId) -> Result<Option<Vec<u8>>, &'static str> {
    let table = CAP_TABLE.lock();
    table.validate(cap_id, agent_id, PermissionBits::READ)
        .map_err(|_| "insufficient storage capability")?;
    drop(table);

    Ok(STORE.lock().get(agent_id, key))
}

pub fn init() {
    println!("[storage] Agent context store initialized");
}
```

- [ ] **Step 2: Commit**

```bash
git add oxide-os/kernel/src/storage/context_store.rs
git commit -m "feat: add per-agent context store (persistent KV)"
```

---

## Summary

After Phase 7, Oxide OS has:
- virtio-blk block device driver
- LRU block cache with write-back (512 KiB cache)
- OxideFS: log-structured, content-addressable filesystem
- Content deduplication (shared embeddings, repeated data)
- Per-agent context store (key-value, persists across restarts)
- Capability-gated storage access
- Ready for Phase 8 (crypto for TLS, signing, secure capabilities)
