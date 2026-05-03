use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;
use super::virtio_blk::DEVICE;
use crate::println;

const CACHE_SIZE: usize = 256; // Cache up to 256 blocks (128 KiB)

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
        BlockCache { cache: BTreeMap::new(), access_counter: 0 }
    }

    pub fn read(&mut self, block_num: u64) -> Result<[u8; 512], &'static str> {
        self.access_counter += 1;
        if let Some(cached) = self.cache.get_mut(&block_num) {
            cached.access_count = self.access_counter;
            return Ok(cached.data);
        }
        let mut data = [0u8; 512];
        DEVICE.lock().read_block(block_num, &mut data)?;
        if self.cache.len() >= CACHE_SIZE { self.evict_one(); }
        self.cache.insert(block_num, CachedBlock { data, dirty: false, access_count: self.access_counter });
        Ok(data)
    }

    pub fn write(&mut self, block_num: u64, data: [u8; 512]) -> Result<(), &'static str> {
        self.access_counter += 1;
        if self.cache.len() >= CACHE_SIZE && !self.cache.contains_key(&block_num) { self.evict_one(); }
        self.cache.insert(block_num, CachedBlock { data, dirty: true, access_count: self.access_counter });
        Ok(())
    }

    pub fn flush(&mut self) -> Result<usize, &'static str> {
        let dirty: Vec<(u64, [u8; 512])> = self.cache.iter()
            .filter(|(_, b)| b.dirty).map(|(&n, b)| (n, b.data)).collect();
        let device = DEVICE.lock();
        for (num, data) in &dirty { device.write_block(*num, data)?; }
        let count = dirty.len();
        for (num, _) in &dirty {
            if let Some(b) = self.cache.get_mut(num) { b.dirty = false; }
        }
        Ok(count)
    }

    fn evict_one(&mut self) {
        let lru = self.cache.iter().min_by_key(|(_, b)| b.access_count).map(|(&n, _)| n);
        if let Some(num) = lru {
            if let Some(block) = self.cache.get(&num) {
                if block.dirty { let _ = DEVICE.lock().write_block(num, &block.data); }
            }
            self.cache.remove(&num);
        }
    }
}

pub static CACHE: Mutex<BlockCache> = Mutex::new(BlockCache::new());

pub fn init() {
    println!("[storage] Block cache: {} blocks max ({} KiB)", CACHE_SIZE, CACHE_SIZE * 512 / 1024);
}
