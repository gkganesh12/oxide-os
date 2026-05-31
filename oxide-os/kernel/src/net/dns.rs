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

    pub fn resolve(&self, hostname: &str) -> Option<Ipv4Address> {
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
    // Dotted-quad shortcut: parse "10.0.2.2" directly instead of cache lookup.
    if let Some(addr) = parse_dotted_quad(hostname) {
        return Some(addr);
    }
    RESOLVER.lock().as_mut()?.resolve(hostname)
}

fn parse_dotted_quad(s: &str) -> Option<Ipv4Address> {
    let mut parts = s.split('.');
    let a: u8 = parts.next()?.parse().ok()?;
    let b: u8 = parts.next()?.parse().ok()?;
    let c: u8 = parts.next()?.parse().ok()?;
    let d: u8 = parts.next()?.parse().ok()?;
    if parts.next().is_some() { return None; }
    Some(Ipv4Address::new(a, b, c, d))
}
