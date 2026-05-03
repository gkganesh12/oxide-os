use core::sync::atomic::{AtomicU16, Ordering};
use smoltcp::iface::SocketHandle;
use smoltcp::socket::tcp::{Socket as TcpSocket, SocketBuffer};
use smoltcp::wire::{IpEndpoint, Ipv4Address};
use alloc::vec;
use super::stack::{INTERFACE, SOCKETS};
use crate::capability::{CapId, CAP_TABLE, PermissionBits};
use crate::task::TaskId;

static NEXT_LOCAL_PORT: AtomicU16 = AtomicU16::new(49152);

#[derive(Debug)]
pub enum SocketError {
    ConnectionFailed,
    NotConnected,
    SendFailed,
    ReceiveFailed,
    CapabilityDenied,
}

pub fn tcp_create() -> Option<SocketHandle> {
    let rx_buffer = SocketBuffer::new(vec![0; 65535]);
    let tx_buffer = SocketBuffer::new(vec![0; 65535]);
    let socket = TcpSocket::new(rx_buffer, tx_buffer);
    let mut sockets = SOCKETS.lock();
    sockets.as_mut().map(|set| set.add(socket))
}

pub fn tcp_connect(
    handle: SocketHandle,
    remote_addr: Ipv4Address,
    remote_port: u16,
    task_id: TaskId,
    net_cap: CapId,
) -> Result<(), SocketError> {
    {
        let table = CAP_TABLE.lock();
        table.validate(net_cap, task_id, PermissionBits::CONNECT)
            .map_err(|_| SocketError::CapabilityDenied)?;
    }
    let mut sockets = SOCKETS.lock();
    let mut iface = INTERFACE.lock();
    if let (Some(sockets), Some(iface)) = (sockets.as_mut(), iface.as_mut()) {
        let socket = sockets.get_mut::<TcpSocket>(handle);
        let remote = IpEndpoint::new(remote_addr.into(), remote_port);
        let local_port = NEXT_LOCAL_PORT.fetch_add(1, Ordering::Relaxed);
        socket.connect(iface.context(), remote, local_port)
            .map_err(|_| SocketError::ConnectionFailed)?;
        Ok(())
    } else {
        Err(SocketError::ConnectionFailed)
    }
}

pub fn tcp_send(handle: SocketHandle, data: &[u8]) -> Result<usize, SocketError> {
    let mut sockets = SOCKETS.lock();
    if let Some(sockets) = sockets.as_mut() {
        let socket = sockets.get_mut::<TcpSocket>(handle);
        if !socket.may_send() { return Err(SocketError::NotConnected); }
        socket.send_slice(data).map_err(|_| SocketError::SendFailed)
    } else {
        Err(SocketError::SendFailed)
    }
}

pub fn tcp_receive(handle: SocketHandle, buffer: &mut [u8]) -> Result<usize, SocketError> {
    let mut sockets = SOCKETS.lock();
    if let Some(sockets) = sockets.as_mut() {
        let socket = sockets.get_mut::<TcpSocket>(handle);
        if !socket.may_recv() { return Err(SocketError::NotConnected); }
        socket.recv_slice(buffer).map_err(|_| SocketError::ReceiveFailed)
    } else {
        Err(SocketError::ReceiveFailed)
    }
}

pub fn tcp_close(handle: SocketHandle) {
    let mut sockets = SOCKETS.lock();
    if let Some(sockets) = sockets.as_mut() {
        let socket = sockets.get_mut::<TcpSocket>(handle);
        socket.close();
    }
}
