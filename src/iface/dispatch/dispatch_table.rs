use std::collections::{btree_map::Entry as MapEntry, BTreeMap, BTreeSet};

use crate::{
    iface::{Context, SocketHandle, SocketSet},
    socket::{AnySocket, Socket},
    wire::{IpEndpoint, IpListenEndpoint, IpProtocol, IpRepr, IpVersion},
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

#[derive(Debug, Default)]
struct TcpLocalEndpoint {
    listen_sockets: BTreeSet<SocketHandle>,
    established_sockets: BTreeMap<IpEndpoint, SocketHandle>,
}

impl TcpLocalEndpoint {
    pub fn new() -> TcpLocalEndpoint {
        TcpLocalEndpoint {
            listen_sockets: BTreeSet::new(),
            established_sockets: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
pub struct DispatchTable {
    raw: BTreeMap<(IpVersion, IpProtocol), SocketHandle>,
    udp: BTreeMap<IpListenEndpoint, SocketHandle>,
    tcp: BTreeMap<IpListenEndpoint, TcpLocalEndpoint>,

    rev_raw: BTreeMap<SocketHandle, (IpVersion, IpProtocol)>,
    rev_udp: BTreeMap<SocketHandle, IpListenEndpoint>,
    rev_tcp: BTreeMap<SocketHandle, (IpListenEndpoint, Option<IpEndpoint>)>,
}

pub type WithHandle<'a, T> = Option<(&'a mut T, SocketHandle)>;

#[derive(Debug)]
pub enum AddError {
    AlreadyInUse,
}

#[derive(Debug)]
pub enum RemoveError {
    SocketNotFound,
}

impl DispatchTable {
    pub(crate) fn new() -> DispatchTable {
        DispatchTable {
            raw: BTreeMap::new(),
            tcp: BTreeMap::new(),
            udp: BTreeMap::new(),
            rev_raw: BTreeMap::new(),
            rev_tcp: BTreeMap::new(),
            rev_udp: BTreeMap::new(),
        }
    }

    pub(crate) fn add_socket(
        &mut self,
        socket: &Socket,
        handle: SocketHandle,
    ) -> Result<(), AddError> {
        match *socket {
            Socket::Udp(ref udp_socket) => self.add_udp_socket(udp_socket, handle),
            Socket::Tcp(ref tcp_socket) => self.add_tcp_socket(tcp_socket, handle),
            Socket::Raw(ref raw_socket) => self.add_raw_socket(raw_socket, handle),
            _ => Ok(()),
        }
    }

    #[allow(dead_code)]
    #[cfg(feature = "socket-udp")]
    pub(crate) fn add_udp_socket(
        &mut self,
        udp_socket: &UdpSocket,
        handle: SocketHandle,
    ) -> Result<(), AddError> {
        if !udp_socket.endpoint().is_specified() {
            return Ok(());
        }
        match (
            self.udp.entry(udp_socket.endpoint()),
            self.rev_udp.entry(handle),
        ) {
            (MapEntry::Vacant(e), MapEntry::Vacant(re)) => {
                e.insert(handle);
                re.insert(udp_socket.endpoint());
            }
            _ => return Err(AddError::AlreadyInUse),
        };
        Ok(())
    }

    #[allow(dead_code)]
    #[cfg(feature = "socket-raw")]
    pub(crate) fn add_raw_socket(
        &mut self,
        raw_socket: &RawSocket,
        handle: SocketHandle,
    ) -> Result<(), AddError> {
        let key = (raw_socket.ip_version(), raw_socket.ip_protocol());
        match (self.raw.entry(key), self.rev_raw.entry(handle)) {
            (MapEntry::Vacant(e), MapEntry::Vacant(re)) => {
                e.insert(handle);
                re.insert(key);
            }
            _ => return Err(AddError::AlreadyInUse),
        };
        Ok(())
    }

    #[allow(dead_code)]
    #[cfg(feature = "socket-tcp")]
    pub(crate) fn add_tcp_socket(
        &mut self,
        tcp_socket: &TcpSocket,
        handle: SocketHandle,
    ) -> Result<(), AddError> {
        let Some(listen_endpoint) = tcp_socket
            .listen_endpoint()
            .or_else(|| tcp_socket.local_endpoint().map(Into::into))
        else {
            return Ok(());
        };

        let rev_entry = match self.rev_tcp.entry(handle) {
            MapEntry::Occupied(_) => return Err(AddError::AlreadyInUse),
            MapEntry::Vacant(e) => e,
        };

        let tcp_endpoint = self
            .tcp
            .entry(listen_endpoint)
            .or_insert_with(TcpLocalEndpoint::default);

        if let Some(remote_endpoint) = tcp_socket.remote_endpoint() {
            // socket established
            match tcp_endpoint.established_sockets.entry(remote_endpoint) {
                MapEntry::Occupied(_) => return Err(AddError::AlreadyInUse),
                MapEntry::Vacant(e) => {
                    e.insert(handle);
                    rev_entry.insert((listen_endpoint, Some(remote_endpoint)));
                }
            };
        } else {
            // socket not established
            tcp_endpoint.listen_sockets.insert(handle);
            rev_entry.insert((listen_endpoint, None));
        }

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
    pub(crate) fn remove_udp_socket(&mut self, handle: SocketHandle) -> Result<(), RemoveError> {
        match self.rev_udp.entry(handle) {
            MapEntry::Vacant(_) => Err(RemoveError::SocketNotFound),
            MapEntry::Occupied(re) => match self.udp.entry(*re.get()) {
                MapEntry::Vacant(_) => Err(RemoveError::SocketNotFound),
                MapEntry::Occupied(e) => {
                    e.remove();
                    re.remove();
                    Ok(())
                }
            },
        }
    }

    #[allow(dead_code)]
    #[cfg(feature = "socket-raw")]
    pub(crate) fn remove_raw_socket(&mut self, handle: SocketHandle) -> Result<(), RemoveError> {
        match self.rev_raw.entry(handle) {
            MapEntry::Vacant(_) => Err(RemoveError::SocketNotFound),
            MapEntry::Occupied(re) => match self.raw.entry(*re.get()) {
                MapEntry::Vacant(_) => Err(RemoveError::SocketNotFound),
                MapEntry::Occupied(e) => {
                    e.remove();
                    re.remove();
                    Ok(())
                }
            },
        }
    }

    #[allow(dead_code)]
    #[cfg(feature = "socket-tcp")]
    pub(crate) fn remove_tcp_socket(&mut self, handle: SocketHandle) -> Result<(), RemoveError> {
        let re = match self.rev_tcp.entry(handle) {
            MapEntry::Vacant(_) => return Err(RemoveError::SocketNotFound),
            MapEntry::Occupied(re) => re,
        };

        let &(local_endpoint, remote_endpoint) = re.get();

        let mut tc_endpoint_entry = match self.tcp.entry(local_endpoint) {
            MapEntry::Vacant(_) => return Err(RemoveError::SocketNotFound),
            MapEntry::Occupied(e) => e,
        };

        {
            let tcp_endpoint = tc_endpoint_entry.get_mut();

            if let Some(remote_endpoint) = remote_endpoint {
                // socket bound
                match tcp_endpoint.established_sockets.entry(remote_endpoint) {
                    MapEntry::Occupied(o) => {
                        o.remove();
                        re.remove();
                    }
                    MapEntry::Vacant(_) => return Err(RemoveError::SocketNotFound),
                }
            } else {
                //socket unbound
                if tcp_endpoint.listen_sockets.remove(&handle) {
                    re.remove();
                } else {
                    return Err(RemoveError::SocketNotFound);
                }
            }
        }

        if tc_endpoint_entry.get().listen_sockets.is_empty()
            && tc_endpoint_entry.get().established_sockets.is_empty()
        {
            tc_endpoint_entry.remove();
        }

        Ok(())
    }

    #[cfg(feature = "socket-udp")]
    pub(crate) fn get_udp_socket<'a: 'b, 'b>(
        &self,
        _cx: &mut Context,
        set: &'b mut SocketSet<'a>,
        ip_repr: &IpRepr,
        udp_repr: &UdpRepr,
    ) -> WithHandle<'b, UdpSocket<'a>> {
        use crate::wire::{IpAddress, Ipv4Address, Ipv6Address};

        let handle = self
            .udp
            .get(&IpListenEndpoint::from(IpEndpoint::new(
                // bound address and port
                ip_repr.dst_addr(),
                udp_repr.dst_port,
            )))
            .or_else(|| self.udp.get(&IpListenEndpoint::from(udp_repr.dst_port))) // bound port only
            .or_else(|| {
                self.udp.get(&IpListenEndpoint::from(IpEndpoint::new(
                    // bound port and ip version only
                    match ip_repr.dst_addr().version() {
                        IpVersion::Ipv4 => IpAddress::Ipv4(Ipv4Address::UNSPECIFIED),
                        IpVersion::Ipv6 => IpAddress::Ipv6(Ipv6Address::UNSPECIFIED),
                    },
                    udp_repr.dst_port,
                )))
            })
            .copied()?;
        let socket = set.get_mut::<UdpSocket>(handle);
        Some((socket, handle))
    }

    #[cfg(feature = "socket-raw")]
    pub(crate) fn get_raw_socket<'a: 'b, 'b>(
        &self,
        set: &'b mut SocketSet<'a>,
        ip_repr: &IpRepr,
    ) -> WithHandle<'b, RawSocket<'a>> {
        let key = (ip_repr.version(), ip_repr.next_header());
        let handle = self.raw.get(&key).copied()?;
        let socket = set.get_mut::<RawSocket>(handle);
        Some((socket, handle))
    }

    #[cfg(feature = "socket-tcp")]
    pub(crate) fn get_tcp_socket<'a: 'b, 'b>(
        &self,
        _cx: &mut crate::iface::Context,
        set: &'b mut SocketSet<'a>,
        ip_repr: &IpRepr,
        tcp_repr: &TcpRepr,
    ) -> WithHandle<'b, TcpSocket<'a>> {
        use crate::wire::{IpAddress, Ipv4Address, Ipv6Address};

        let local_endpoint = self
            .tcp
            .get(&IpListenEndpoint::from(IpEndpoint::new(
                // bound address and port
                ip_repr.dst_addr(),
                tcp_repr.dst_port,
            )))
            .or_else(|| self.tcp.get(&IpListenEndpoint::from(tcp_repr.dst_port))) // bound port only
            .or_else(|| {
                self.tcp.get(&IpListenEndpoint::from(IpEndpoint::new(
                    // bound port and ip version only
                    match ip_repr.dst_addr().version() {
                        IpVersion::Ipv4 => IpAddress::Ipv4(Ipv4Address::UNSPECIFIED),
                        IpVersion::Ipv6 => IpAddress::Ipv6(Ipv6Address::UNSPECIFIED),
                    },
                    tcp_repr.dst_port,
                )))
            })?;

        let handle = local_endpoint
            .established_sockets
            .get(&IpEndpoint::new(ip_repr.src_addr(), tcp_repr.src_port))
            .or_else(|| local_endpoint.listen_sockets.iter().next())
            .copied()?;

        let socket = set.get_mut::<TcpSocket>(handle);

        Some((socket, handle))
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
