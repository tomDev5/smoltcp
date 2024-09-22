use crate::{
    iface::{Context, SocketHandle, SocketSet},
    socket::{AnySocket, Socket},
    wire::IpRepr,
};

#[cfg(feature = "socket-udp")]
use crate::{socket::udp::Socket as UdpSocket, wire::UdpRepr};

#[cfg(feature = "socket-raw")]
use crate::socket::raw::Socket as RawSocket;

#[cfg(feature = "socket-tcp")]
use crate::{socket::tcp::Socket as TcpSocket, wire::TcpRepr};

#[cfg(feature = "socket-dhcpv4")]
use crate::socket::dhcpv4::Socket as Dhcpv4Socket;

#[cfg(feature = "socket-dns")]
use crate::socket::dns::Socket as DnsSocket;

#[cfg(feature = "socket-icmp")]
use crate::socket::icmp::Socket as IcmpSocket;

#[cfg(all(feature = "socket-icmp", feature = "proto-ipv4"))]
use crate::wire::{Icmpv4Repr, Ipv4Repr};

#[cfg(all(feature = "socket-icmp", feature = "proto-ipv6"))]
use crate::wire::{Icmpv6Repr, Ipv6Repr};

pub type WithHandle<'a, T> = Option<(&'a mut T, SocketHandle)>;

#[derive(Debug)]
pub struct DispatchTable {}

#[derive(Debug)]
pub enum AddError {}

#[derive(Debug)]
pub enum RemoveError {}

impl DispatchTable {
    pub(crate) fn new() -> DispatchTable {
        DispatchTable {}
    }

    pub(crate) fn add_socket(
        &mut self,
        _socket: &Socket,
        _handle: SocketHandle,
    ) -> Result<(), AddError> {
        Ok(())
    }

    #[allow(dead_code)]
    #[cfg(feature = "socket-udp")]
    pub(crate) fn add_udp_socket(
        &mut self,
        _udp_socket: &UdpSocket,
        _handle: SocketHandle,
    ) -> Result<(), AddError> {
        Ok(())
    }

    #[allow(dead_code)]
    #[cfg(feature = "socket-raw")]
    pub(crate) fn add_raw_socket(
        &mut self,
        _raw_socket: &RawSocket,
        _handle: SocketHandle,
    ) -> Result<(), AddError> {
        Ok(())
    }

    #[allow(dead_code)]
    #[cfg(feature = "socket-tcp")]
    pub(crate) fn add_tcp_socket(
        &mut self,
        _tcp_socket: &TcpSocket,
        _handle: SocketHandle,
    ) -> Result<(), AddError> {
        Ok(())
    }

    pub(crate) fn remove_socket(
        &mut self,
        _socket: &Socket,
        _handle: SocketHandle,
    ) -> Result<(), RemoveError> {
        Ok(())
    }

    #[allow(dead_code)]
    #[cfg(feature = "socket-udp")]
    pub(crate) fn remove_udp_socket(&mut self, _handle: SocketHandle) -> Result<(), RemoveError> {
        Ok(())
    }

    #[allow(dead_code)]
    #[cfg(feature = "socket-raw")]
    pub(crate) fn remove_raw_socket(&mut self, _handle: SocketHandle) -> Result<(), RemoveError> {
        Ok(())
    }

    #[allow(dead_code)]
    #[cfg(feature = "socket-tcp")]
    pub(crate) fn remove_tcp_socket(&mut self, _handle: SocketHandle) -> Result<(), RemoveError> {
        Ok(())
    }

    #[cfg(feature = "socket-udp")]
    pub(crate) fn get_udp_socket<'a: 'b, 'b>(
        &self,
        cx: &mut Context,
        set: &'b mut SocketSet<'a>,
        ip_repr: &IpRepr,
        udp_repr: &UdpRepr,
    ) -> WithHandle<'b, UdpSocket<'a>> {
        for (handle, socket) in set.iter_mut() {
            if let Some(udp_socket) = UdpSocket::downcast_mut(socket) {
                if udp_socket.accepts(cx, ip_repr, udp_repr) {
                    return Some((udp_socket, handle));
                }
            }
        }
        None
    }

    #[cfg(feature = "socket-raw")]
    pub(crate) fn get_raw_socket<'a: 'b, 'b>(
        &self,
        set: &'b mut SocketSet<'a>,
        ip_repr: &IpRepr,
    ) -> WithHandle<'b, RawSocket<'a>> {
        for (handle, socket) in set.iter_mut() {
            if let Some(raw_socket) = RawSocket::downcast_mut(socket) {
                if raw_socket.accepts(ip_repr) {
                    return Some((raw_socket, handle));
                }
            }
        }
        None
    }

    #[cfg(feature = "socket-tcp")]
    pub(crate) fn get_tcp_socket<'a: 'b, 'b>(
        &self,
        cx: &mut crate::iface::Context,
        set: &'b mut SocketSet<'a>,
        ip_repr: &IpRepr,
        tcp_repr: &TcpRepr,
    ) -> WithHandle<'b, TcpSocket<'a>> {
        for (handle, socket) in set.iter_mut() {
            if let Some(tcp_socket) = TcpSocket::downcast_mut(socket) {
                if tcp_socket.accepts(cx, ip_repr, tcp_repr) {
                    return Some((tcp_socket, handle));
                }
            }
        }
        None
    }

    #[cfg(feature = "socket-dhcpv4")]
    pub(crate) fn get_dhcpv4_socket<'a: 'b, 'b>(
        &self,
        set: &'b mut SocketSet<'a>,
        udp_repr: &UdpRepr,
    ) -> WithHandle<'b, Dhcpv4Socket<'a>> {
        for (handle, socket) in set.iter_mut() {
            if let Some(dhcpv4_socket) = Dhcpv4Socket::downcast_mut(socket) {
                if dhcpv4_socket.accepts(udp_repr) {
                    return Some((dhcpv4_socket, handle));
                }
            }
        }
        None
    }

    #[cfg(feature = "socket-dns")]
    pub(crate) fn get_dns_socket<'a: 'b, 'b>(
        &self,
        set: &'b mut SocketSet<'a>,
        ip_repr: &IpRepr,
        udp_repr: &UdpRepr,
    ) -> WithHandle<'b, DnsSocket<'a>> {
        for (handle, socket) in set.iter_mut() {
            if let Some(tcp_socket) = DnsSocket::downcast_mut(socket) {
                if tcp_socket.accepts(ip_repr, udp_repr) {
                    return Some((tcp_socket, handle));
                }
            }
        }
        None
    }

    #[cfg(all(feature = "socket-icmp", feature = "proto-ipv4"))]
    pub(crate) fn get_icmpv4_socket<'a: 'b, 'b>(
        &self,
        cx: &mut crate::iface::Context,
        set: &'b mut SocketSet<'a>,
        ip_repr: &Ipv4Repr,
        icmp_repr: &Icmpv4Repr,
    ) -> WithHandle<'b, IcmpSocket<'a>> {
        for (handle, socket) in set.iter_mut() {
            if let Some(icmp_socket) = IcmpSocket::downcast_mut(socket) {
                if icmp_socket.accepts_v4(cx, ip_repr, icmp_repr) {
                    return Some((icmp_socket, handle));
                }
            }
        }
        None
    }

    #[cfg(all(feature = "socket-icmp", feature = "proto-ipv6"))]
    pub(crate) fn get_icmpv6_socket<'a: 'b, 'b>(
        &self,
        cx: &mut crate::iface::Context,
        set: &'b mut SocketSet<'a>,
        ip_repr: &Ipv6Repr,
        icmp_repr: &Icmpv6Repr,
    ) -> WithHandle<'b, IcmpSocket<'a>> {
        for (handle, socket) in set.iter_mut() {
            if let Some(icmp_socket) = IcmpSocket::downcast_mut(socket) {
                if icmp_socket.accepts_v6(cx, ip_repr, icmp_repr) {
                    return Some((icmp_socket, handle));
                }
            }
        }
        None
    }
}
