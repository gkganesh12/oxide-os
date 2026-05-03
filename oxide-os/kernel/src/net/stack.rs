use smoltcp::iface::{Config, Interface, SocketSet, SocketStorage};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpCidr, Ipv4Address};
use spin::Mutex;
use alloc::vec;
use alloc::vec::Vec;
use crate::println;
use super::virtio_net::DEVICE;

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
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.0)
    }
}

pub struct OxideTxToken;

impl TxToken for OxideTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = vec![0u8; len];
        let result = f(&mut buffer);
        let mut dev = DEVICE.lock();
        let _ = dev.transmit(&buffer);
        result
    }
}

pub static INTERFACE: Mutex<Option<Interface>> = Mutex::new(None);
pub static SOCKETS: Mutex<Option<SocketSet<'static>>> = Mutex::new(None);

pub fn init() {
    let dev = DEVICE.lock();
    let mac = dev.mac_address();
    drop(dev);

    let hardware_addr: HardwareAddress = EthernetAddress(mac).into();
    let config = Config::new(hardware_addr);
    let mut iface = Interface::new(config, &mut OxideNetDevice, Instant::from_millis(0));

    iface.update_ip_addrs(|addrs| {
        addrs.push(IpCidr::new(Ipv4Address::new(10, 0, 2, 15).into(), 24)).unwrap();
    });

    iface.routes_mut().add_default_ipv4_route(Ipv4Address::new(10, 0, 2, 2)).unwrap();

    let sockets: SocketSet<'static> = SocketSet::new(Vec::<SocketStorage<'static>>::new());

    *INTERFACE.lock() = Some(iface);
    *SOCKETS.lock() = Some(sockets);

    println!("[net] TCP/IP stack: 10.0.2.15/24, gateway 10.0.2.2");
}

pub fn poll() {
    let mut iface_lock = INTERFACE.lock();
    let mut sockets_lock = SOCKETS.lock();
    if let (Some(iface), Some(sockets)) = (iface_lock.as_mut(), sockets_lock.as_mut()) {
        let timestamp = Instant::from_millis(crate::interrupts::ticks() as i64);
        iface.poll(timestamp, &mut OxideNetDevice, sockets);
    }
}
