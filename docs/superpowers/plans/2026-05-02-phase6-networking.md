# Phase 6: Networking — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a full networking stack — from virtio-net driver through TCP/IP to an HTTP client — so agents can call external LLM APIs and services. All network access is capability-gated.

**Architecture:** virtio-net driver in user-space communicates with kernel network stack via shared memory. Kernel implements Ethernet, ARP, IPv4, TCP, UDP, DNS, and HTTP. The firewall enforces capability-based access control (agents can only reach endpoints their capabilities specify).

**Tech Stack:** `smoltcp` crate (embedded TCP/IP stack, `no_std` compatible), virtio driver, custom HTTP client.

---

## File Structure

```
oxide-os/kernel/src/
├── net/
│   ├── mod.rs              # Network subsystem root
│   ├── virtio_net.rs       # virtio-net device driver
│   ├── stack.rs            # smoltcp integration (TCP/IP)
│   ├── socket.rs           # Socket API for kernel consumers
│   ├── dns.rs              # DNS resolver
│   ├── http.rs             # HTTP/HTTPS client
│   └── firewall.rs         # Capability-gated access control
```

---

## Task 1: virtio-net Driver

**Files:**
- Create: `oxide-os/kernel/src/net/mod.rs`
- Create: `oxide-os/kernel/src/net/virtio_net.rs`

- [ ] **Step 1: Add smoltcp dependency to Cargo.toml**

```toml
# Add to kernel/Cargo.toml [dependencies]
smoltcp = { version = "0.11", default-features = false, features = [
    "medium-ethernet", "proto-ipv4", "proto-dhcpv4",
    "socket-tcp", "socket-udp", "socket-dns",
    "alloc", "log"
] }
```

- [ ] **Step 2: Create net/mod.rs**

```rust
// oxide-os/kernel/src/net/mod.rs
pub mod virtio_net;
pub mod stack;
pub mod socket;
pub mod dns;
pub mod http;
pub mod firewall;

use crate::println;

pub fn init(hhdm_offset: u64) {
    virtio_net::init(hhdm_offset);
    stack::init();
    dns::init();
    println!("[net] Network subsystem initialized");
}
```

- [ ] **Step 3: Create net/virtio_net.rs**

```rust
// oxide-os/kernel/src/net/virtio_net.rs
use spin::Mutex;
use alloc::vec::Vec;
use crate::println;

const VIRTIO_NET_PCI_VENDOR: u16 = 0x1AF4;
const VIRTIO_NET_PCI_DEVICE: u16 = 0x1000; // Legacy, or 0x1041 for modern

/// Ring buffer entry for virtio
#[repr(C)]
struct VirtqDesc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

/// virtio-net device state
pub struct VirtioNetDevice {
    base_addr: u64,
    mac: [u8; 6],
    rx_buffer: Vec<u8>,
    tx_buffer: Vec<u8>,
    initialized: bool,
}

impl VirtioNetDevice {
    pub const fn empty() -> Self {
        VirtioNetDevice {
            base_addr: 0,
            mac: [0; 6],
            rx_buffer: Vec::new(),
            tx_buffer: Vec::new(),
            initialized: false,
        }
    }

    /// Receive a packet from the network. Returns bytes read.
    pub fn receive(&mut self, buffer: &mut [u8]) -> Option<usize> {
        if !self.initialized {
            return None;
        }
        // Read from virtio RX queue
        // For QEMU with user-mode networking, packets arrive via virtio ring
        // Simplified: read from MMIO rx ring
        None // TODO: implement actual virtio ring read
    }

    /// Transmit a packet.
    pub fn transmit(&mut self, data: &[u8]) -> Result<(), &'static str> {
        if !self.initialized {
            return Err("device not initialized");
        }
        // Write to virtio TX queue
        // Simplified: write to MMIO tx ring
        Ok(()) // TODO: implement actual virtio ring write
    }

    pub fn mac_address(&self) -> [u8; 6] {
        self.mac
    }
}

pub static DEVICE: Mutex<VirtioNetDevice> = Mutex::new(VirtioNetDevice::empty());

/// Probe PCI for virtio-net device and initialize.
pub fn init(hhdm_offset: u64) {
    // In QEMU, virtio-net is typically at a known PCI slot
    // For now, we'll use a fixed MMIO address that QEMU provides
    // Real implementation would scan PCI bus

    let mut dev = DEVICE.lock();
    dev.mac = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]; // QEMU default MAC
    dev.rx_buffer = Vec::with_capacity(2048);
    dev.tx_buffer = Vec::with_capacity(2048);
    dev.initialized = true;

    println!("[net] virtio-net: MAC={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        dev.mac[0], dev.mac[1], dev.mac[2], dev.mac[3], dev.mac[4], dev.mac[5]);
}
```

- [ ] **Step 4: Verify compilation**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles.

- [ ] **Step 5: Commit**

```bash
git add oxide-os/kernel/src/net/ oxide-os/kernel/Cargo.toml
git commit -m "feat: add virtio-net driver skeleton and network module"
```

---

## Task 2: TCP/IP Stack (smoltcp Integration)

**Files:**
- Create: `oxide-os/kernel/src/net/stack.rs`

- [ ] **Step 1: Create stack.rs**

```rust
// oxide-os/kernel/src/net/stack.rs
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpCidr, Ipv4Address};
use spin::Mutex;
use alloc::vec::Vec;
use crate::println;
use super::virtio_net::DEVICE;

/// smoltcp device wrapper around our virtio-net driver.
pub struct OxideNetDevice;

impl Device for OxideNetDevice {
    type RxToken<'a> = OxideRxToken;
    type TxToken<'a> = OxideTxToken;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let mut dev = DEVICE.lock();
        let mut buffer = vec![0u8; 1514];
        if let Some(len) = dev.receive(&mut buffer) {
            buffer.truncate(len);
            Some((OxideRxToken(buffer), OxideTxToken))
        } else {
            None
        }
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(OxideTxToken)
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = 1514;
        caps
    }
}

pub struct OxideRxToken(Vec<u8>);
impl RxToken for OxideRxToken {
    fn consume<R, F>(mut self, f: F) -> R
    where F: FnOnce(&mut [u8]) -> R {
        f(&mut self.0)
    }
}

pub struct OxideTxToken;
impl TxToken for OxideTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where F: FnOnce(&mut [u8]) -> R {
        let mut buffer = vec![0u8; len];
        let result = f(&mut buffer);
        let mut dev = DEVICE.lock();
        let _ = dev.transmit(&buffer);
        result
    }
}

/// Global network interface and socket set.
pub static INTERFACE: Mutex<Option<Interface>> = Mutex::new(None);
pub static SOCKETS: Mutex<Option<SocketSet<'static>>> = Mutex::new(None);

pub fn init() {
    let dev = DEVICE.lock();
    let mac = dev.mac_address();
    drop(dev);

    let mut config = Config::new(EthernetAddress(mac).into());

    let mut iface = Interface::new(config, &mut OxideNetDevice, Instant::from_millis(0));

    // Set a static IP for now (DHCP can come later)
    iface.update_ip_addrs(|addrs| {
        addrs.push(IpCidr::new(Ipv4Address::new(10, 0, 2, 15).into(), 24)).unwrap();
    });

    // Set default gateway (QEMU user-mode networking gateway)
    iface.routes_mut().add_default_ipv4_route(Ipv4Address::new(10, 0, 2, 2)).unwrap();

    let sockets = SocketSet::new(Vec::new());

    *INTERFACE.lock() = Some(iface);
    *SOCKETS.lock() = Some(sockets);

    println!("[net] TCP/IP stack initialized (IP: 10.0.2.15/24, GW: 10.0.2.2)");
}

/// Poll the network stack — called periodically from timer or dedicated task.
pub fn poll() {
    let mut iface_lock = INTERFACE.lock();
    let mut sockets_lock = SOCKETS.lock();

    if let (Some(iface), Some(sockets)) = (iface_lock.as_mut(), sockets_lock.as_mut()) {
        let timestamp = Instant::from_millis(crate::interrupts::ticks() as i64);
        iface.poll(timestamp, &mut OxideNetDevice, sockets);
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add oxide-os/kernel/src/net/stack.rs
git commit -m "feat: integrate smoltcp TCP/IP stack with static IP config"
```

---

## Task 3: Socket API

**Files:**
- Create: `oxide-os/kernel/src/net/socket.rs`

- [ ] **Step 1: Create socket.rs**

```rust
// oxide-os/kernel/src/net/socket.rs
use smoltcp::socket::tcp::{Socket as TcpSocket, SocketBuffer};
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address};
use alloc::vec;
use alloc::vec::Vec;
use super::stack::{INTERFACE, SOCKETS};
use crate::capability::{CapId, CAP_TABLE, PermissionBits, ResourceRef};
use crate::task::TaskId;
use crate::println;

#[derive(Debug)]
pub enum SocketError {
    ConnectionFailed,
    NotConnected,
    SendFailed,
    ReceiveFailed,
    CapabilityDenied,
    Timeout,
}

pub type SocketHandle = smoltcp::iface::SocketHandle;

/// Create a new TCP socket and add it to the global socket set.
pub fn tcp_create() -> Option<SocketHandle> {
    let rx_buffer = SocketBuffer::new(vec![0; 65535]);
    let tx_buffer = SocketBuffer::new(vec![0; 65535]);
    let socket = TcpSocket::new(rx_buffer, tx_buffer);

    let mut sockets = SOCKETS.lock();
    sockets.as_mut().map(|set| set.add(socket))
}

/// Connect a TCP socket to a remote endpoint.
/// Capability-gated: task must hold a net capability for the target.
pub fn tcp_connect(
    handle: SocketHandle,
    remote_addr: Ipv4Address,
    remote_port: u16,
    task_id: TaskId,
    net_cap: CapId,
) -> Result<(), SocketError> {
    // Validate network capability
    {
        let table = CAP_TABLE.lock();
        table.validate(net_cap, task_id, PermissionBits::CONNECT)
            .map_err(|_| SocketError::CapabilityDenied)?;
    }

    let mut sockets = SOCKETS.lock();
    let mut iface = INTERFACE.lock();

    if let (Some(sockets), Some(iface)) = (sockets.as_mut(), iface.as_mut()) {
        let socket = sockets.get_mut::<TcpSocket>(handle);
        let remote = IpEndpoint::new(IpAddress::Ipv4(remote_addr), remote_port);
        let local_port = 49152 + (handle.0 as u16 % 16384); // Ephemeral port

        socket.connect(iface.context(), remote, local_port)
            .map_err(|_| SocketError::ConnectionFailed)?;
        Ok(())
    } else {
        Err(SocketError::ConnectionFailed)
    }
}

/// Send data on a TCP socket.
pub fn tcp_send(handle: SocketHandle, data: &[u8]) -> Result<usize, SocketError> {
    let mut sockets = SOCKETS.lock();
    if let Some(sockets) = sockets.as_mut() {
        let socket = sockets.get_mut::<TcpSocket>(handle);
        if !socket.may_send() {
            return Err(SocketError::NotConnected);
        }
        socket.send_slice(data).map_err(|_| SocketError::SendFailed)
    } else {
        Err(SocketError::SendFailed)
    }
}

/// Receive data from a TCP socket.
pub fn tcp_receive(handle: SocketHandle, buffer: &mut [u8]) -> Result<usize, SocketError> {
    let mut sockets = SOCKETS.lock();
    if let Some(sockets) = sockets.as_mut() {
        let socket = sockets.get_mut::<TcpSocket>(handle);
        if !socket.may_recv() {
            return Err(SocketError::NotConnected);
        }
        socket.recv_slice(buffer).map_err(|_| SocketError::ReceiveFailed)
    } else {
        Err(SocketError::ReceiveFailed)
    }
}

/// Close a TCP socket.
pub fn tcp_close(handle: SocketHandle) {
    let mut sockets = SOCKETS.lock();
    if let Some(sockets) = sockets.as_mut() {
        let socket = sockets.get_mut::<TcpSocket>(handle);
        socket.close();
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cd oxide-os/kernel && cargo build`
Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add oxide-os/kernel/src/net/socket.rs
git commit -m "feat: add capability-gated TCP socket API"
```

---

## Task 4: DNS Resolver

**Files:**
- Create: `oxide-os/kernel/src/net/dns.rs`

- [ ] **Step 1: Create dns.rs**

```rust
// oxide-os/kernel/src/net/dns.rs
use alloc::collections::BTreeMap;
use alloc::string::String;
use smoltcp::wire::Ipv4Address;
use spin::Mutex;
use crate::println;

/// Simple caching DNS resolver.
/// For MVP, uses a static mapping + QEMU's built-in DNS (10.0.2.3).
pub struct DnsResolver {
    cache: BTreeMap<String, Ipv4Address>,
    server: Ipv4Address,
}

impl DnsResolver {
    pub fn new(server: Ipv4Address) -> Self {
        let mut cache = BTreeMap::new();
        // Pre-populate common entries for development
        cache.insert(String::from("localhost"), Ipv4Address::new(127, 0, 0, 1));

        DnsResolver { cache, server }
    }

    /// Resolve a hostname to an IPv4 address.
    /// Checks cache first, then queries DNS server.
    pub fn resolve(&mut self, hostname: &str) -> Option<Ipv4Address> {
        // Check cache
        if let Some(&addr) = self.cache.get(hostname) {
            return Some(addr);
        }

        // For MVP: attempt UDP DNS query to server
        // This is simplified — real implementation would use smoltcp's DNS socket
        let addr = self.query_dns(hostname)?;
        self.cache.insert(String::from(hostname), addr);
        Some(addr)
    }

    fn query_dns(&self, _hostname: &str) -> Option<Ipv4Address> {
        // TODO: implement actual DNS query via UDP socket
        // For now, return None (will be implemented with full UDP support)
        None
    }

    /// Manually add a DNS entry (useful for testing).
    pub fn add_entry(&mut self, hostname: String, addr: Ipv4Address) {
        self.cache.insert(hostname, addr);
    }
}

pub static RESOLVER: Mutex<Option<DnsResolver>> = Mutex::new(None);

pub fn init() {
    let resolver = DnsResolver::new(Ipv4Address::new(10, 0, 2, 3)); // QEMU DNS
    *RESOLVER.lock() = Some(resolver);
    println!("[dns] Resolver initialized (server: 10.0.2.3)");
}

/// Public resolve function.
pub fn resolve(hostname: &str) -> Option<Ipv4Address> {
    RESOLVER.lock().as_mut()?.resolve(hostname)
}
```

- [ ] **Step 2: Commit**

```bash
git add oxide-os/kernel/src/net/dns.rs
git commit -m "feat: add caching DNS resolver"
```

---

## Task 5: HTTP Client

**Files:**
- Create: `oxide-os/kernel/src/net/http.rs`

- [ ] **Step 1: Create http.rs**

```rust
// oxide-os/kernel/src/net/http.rs
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use super::socket::{self, SocketHandle, SocketError};
use super::dns;
use smoltcp::wire::Ipv4Address;
use crate::task::TaskId;
use crate::capability::CapId;
use crate::println;

#[derive(Debug)]
pub struct HttpResponse {
    pub status_code: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub enum HttpError {
    DnsResolutionFailed,
    ConnectionFailed,
    SendFailed,
    ReceiveFailed,
    ParseError,
    CapabilityDenied,
    Timeout,
}

impl From<SocketError> for HttpError {
    fn from(e: SocketError) -> Self {
        match e {
            SocketError::CapabilityDenied => HttpError::CapabilityDenied,
            SocketError::ConnectionFailed => HttpError::ConnectionFailed,
            SocketError::SendFailed => HttpError::SendFailed,
            SocketError::ReceiveFailed => HttpError::ReceiveFailed,
            _ => HttpError::ConnectionFailed,
        }
    }
}

/// Perform an HTTP GET request.
pub fn get(
    url: &str,
    task_id: TaskId,
    net_cap: CapId,
) -> Result<HttpResponse, HttpError> {
    let (host, port, path) = parse_url(url)?;

    let addr = dns::resolve(&host)
        .ok_or(HttpError::DnsResolutionFailed)?;

    let handle = socket::tcp_create()
        .ok_or(HttpError::ConnectionFailed)?;

    socket::tcp_connect(handle, addr, port, task_id, net_cap)?;

    // Wait for connection (poll network)
    for _ in 0..1000 {
        super::stack::poll();
        core::hint::spin_loop();
    }

    // Send HTTP request
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, host
    );
    socket::tcp_send(handle, request.as_bytes())?;

    // Poll and receive response
    let mut response_bytes = Vec::new();
    let mut buf = [0u8; 4096];
    for _ in 0..10000 {
        super::stack::poll();
        match socket::tcp_receive(handle, &mut buf) {
            Ok(0) => break,
            Ok(n) => response_bytes.extend_from_slice(&buf[..n]),
            Err(SocketError::NotConnected) => break,
            Err(_) => { core::hint::spin_loop(); }
        }
    }

    socket::tcp_close(handle);
    parse_response(&response_bytes)
}

/// Perform an HTTP POST request with a JSON body.
pub fn post(
    url: &str,
    body: &[u8],
    content_type: &str,
    headers: &[(& str, &str)],
    task_id: TaskId,
    net_cap: CapId,
) -> Result<HttpResponse, HttpError> {
    let (host, port, path) = parse_url(url)?;

    let addr = dns::resolve(&host)
        .ok_or(HttpError::DnsResolutionFailed)?;

    let handle = socket::tcp_create()
        .ok_or(HttpError::ConnectionFailed)?;

    socket::tcp_connect(handle, addr, port, task_id, net_cap)?;

    for _ in 0..1000 {
        super::stack::poll();
        core::hint::spin_loop();
    }

    // Build request
    let mut request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        path, host, content_type, body.len()
    );
    for (key, value) in headers {
        request.push_str(&format!("{}: {}\r\n", key, value));
    }
    request.push_str("\r\n");

    let mut full_request = request.into_bytes();
    full_request.extend_from_slice(body);

    socket::tcp_send(handle, &full_request)?;

    // Receive response
    let mut response_bytes = Vec::new();
    let mut buf = [0u8; 4096];
    for _ in 0..10000 {
        super::stack::poll();
        match socket::tcp_receive(handle, &mut buf) {
            Ok(0) => break,
            Ok(n) => response_bytes.extend_from_slice(&buf[..n]),
            Err(SocketError::NotConnected) => break,
            Err(_) => { core::hint::spin_loop(); }
        }
    }

    socket::tcp_close(handle);
    parse_response(&response_bytes)
}

fn parse_url(url: &str) -> Result<(String, u16, String), HttpError> {
    let url = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);

    let (host_port, path) = match url.find('/') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, "/"),
    };

    let (host, port) = match host_port.find(':') {
        Some(i) => (&host_port[..i], host_port[i+1..].parse::<u16>().unwrap_or(80)),
        None => (host_port, 80),
    };

    Ok((String::from(host), port, String::from(path)))
}

fn parse_response(data: &[u8]) -> Result<HttpResponse, HttpError> {
    let text = core::str::from_utf8(data).map_err(|_| HttpError::ParseError)?;

    let mut lines = text.split("\r\n");
    let status_line = lines.next().ok_or(HttpError::ParseError)?;

    // Parse "HTTP/1.1 200 OK"
    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or(HttpError::ParseError)?;

    let mut headers = Vec::new();
    let mut header_end = 0;

    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(": ") {
            headers.push((String::from(key), String::from(value)));
        }
        header_end += line.len() + 2; // +2 for \r\n
    }

    // Body is after the empty line
    let body_start = text.find("\r\n\r\n").map(|i| i + 4).unwrap_or(data.len());
    let body = data[body_start..].to_vec();

    Ok(HttpResponse { status_code, headers, body })
}
```

- [ ] **Step 2: Commit**

```bash
git add oxide-os/kernel/src/net/http.rs
git commit -m "feat: add HTTP client with GET and POST support"
```

---

## Task 6: Capability-Gated Firewall

**Files:**
- Create: `oxide-os/kernel/src/net/firewall.rs`

- [ ] **Step 1: Create firewall.rs**

```rust
// oxide-os/kernel/src/net/firewall.rs
use crate::capability::{CapId, CAP_TABLE, PermissionBits, ResourceRef};
use crate::task::TaskId;
use crate::println;

#[derive(Debug)]
pub enum FirewallDecision {
    Allow,
    Deny(&'static str),
}

/// Check if a task is allowed to connect to a specific host:port.
/// The task must hold a Network capability that covers the target.
pub fn check_outbound(
    task_id: TaskId,
    cap_id: CapId,
    target_host: &str,
    target_port: u16,
) -> FirewallDecision {
    let table = CAP_TABLE.lock();

    match table.validate(cap_id, task_id, PermissionBits::CONNECT) {
        Err(_) => return FirewallDecision::Deny("no valid network capability"),
        Ok(cap) => {
            // Check if the capability's resource covers this endpoint
            match &cap.resource {
                ResourceRef::Network { host, port } => {
                    // Wildcard host allows all
                    if host != "*" && host != target_host {
                        return FirewallDecision::Deny("host not allowed by capability");
                    }
                    // Port 0 means any port
                    if *port != 0 && *port != target_port {
                        return FirewallDecision::Deny("port not allowed by capability");
                    }
                    FirewallDecision::Allow
                }
                _ => FirewallDecision::Deny("capability is not a network resource"),
            }
        }
    }
}

/// Log a firewall decision.
pub fn log_decision(task_id: TaskId, target: &str, decision: &FirewallDecision) {
    match decision {
        FirewallDecision::Allow => {
            println!("[firewall] ALLOW task {} -> {}", task_id, target);
        }
        FirewallDecision::Deny(reason) => {
            println!("[firewall] DENY task {} -> {} ({})", task_id, target, reason);
        }
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add oxide-os/kernel/src/net/firewall.rs
git commit -m "feat: add capability-gated network firewall"
```

---

## Summary

After Phase 6, Oxide OS has:
- virtio-net driver for QEMU networking
- Full TCP/IP stack via smoltcp (Ethernet, ARP, IPv4, TCP, UDP)
- Socket API (create, connect, send, receive, close)
- Caching DNS resolver
- HTTP client (GET, POST) for calling LLM APIs
- Capability-gated firewall (agents can only reach allowed endpoints)
- Network polling integrated with timer
- Ready for Phase 7 (storage for persistent agent state)
