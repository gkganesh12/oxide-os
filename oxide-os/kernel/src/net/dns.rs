use alloc::collections::BTreeMap;
use alloc::string::String;
use smoltcp::wire::Ipv4Address;
use spin::Mutex;
use crate::println;

pub struct DnsResolver {
    cache: BTreeMap<String, Ipv4Address>,
}

impl DnsResolver {
    pub fn new() -> Self {
        let mut cache = BTreeMap::new();
        cache.insert(String::from("localhost"), Ipv4Address::new(127, 0, 0, 1));
        DnsResolver { cache }
    }

    pub fn resolve(&mut self, hostname: &str) -> Option<Ipv4Address> {
        self.cache.get(hostname).copied()
    }

    pub fn add_entry(&mut self, hostname: String, addr: Ipv4Address) {
        self.cache.insert(hostname, addr);
    }
}

pub static RESOLVER: Mutex<Option<DnsResolver>> = Mutex::new(None);

pub fn init() {
    *RESOLVER.lock() = Some(DnsResolver::new());
    println!("[dns] Resolver initialized (cache-based)");
}

pub fn resolve(hostname: &str) -> Option<Ipv4Address> {
    RESOLVER.lock().as_mut()?.resolve(hostname)
}
