use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::Included;

use crate::{
    socket::AnySocket,
    wire::{IpAddress, IpEndpoint, IpProtocol, IpVersion},
};

use super::SocketHandle;

#[derive(Debug, Default)]
struct TcpLocalEndpoint {
    listen_sockets: BTreeSet<SocketHandle>,
    established_sockets: BTreeMap<IpEndpoint, SocketHandle>,
}

#[derive(Debug, Default)]
pub struct DispatchTable {
    raw: BTreeMap<(IpVersion, IpProtocol), SocketHandle>,
    udp: BTreeMap<IpEndpoint, SocketHandle>,
    tcp: BTreeMap<IpEndpoint, TcpLocalEndpoint>,

    rev_raw: BTreeMap<SocketHandle, (IpVersion, IpProtocol)>,
    rev_udp: BTreeMap<SocketHandle, IpEndpoint>,
    rev_tcp: BTreeMap<SocketHandle, (IpEndpoint, Option<IpEndpoint>)>,
}

impl DispatchTable {
    pub(crate) fn get_tcp_socket(
        &self,
        ip_repr: &crate::wire::IpRepr,
        tcp_repr: &crate::wire::TcpRepr,
    ) -> Option<SocketHandle> {
        DispatchTable::get_socket_data(
            &self.tcp,
            IpEndpoint::new(ip_repr.dst_addr(), tcp_repr.dst_port),
        )
        .and_then(|tcp_endpoint| {
            tcp_endpoint
                .established_sockets
                .get(&IpEndpoint::new(ip_repr.src_addr(), tcp_repr.src_port))
                .or_else(|| tcp_endpoint.listen_sockets.iter().next())
        })
        .copied()
    }

    pub(crate) fn get_udp_socket(
        &self,
        ip_repr: &crate::wire::IpRepr,
        udp_repr: &crate::wire::UdpRepr,
    ) -> Option<SocketHandle> {
        DispatchTable::get_socket_data(
            &self.udp,
            IpEndpoint::new(ip_repr.dst_addr(), udp_repr.dst_port),
        )
        .copied()
    }

    pub(crate) fn get_raw_socket(
        &self,
        ip_version: crate::wire::IpVersion,
        ip_protocol: crate::wire::IpProtocol,
    ) -> Option<SocketHandle> {
        let key = (ip_version, ip_protocol);
        self.raw.get(&key).copied()
    }

    fn get_socket_data<T>(tree: &BTreeMap<IpEndpoint, T>, endpoint: IpEndpoint) -> Option<&T> {
        let unspecified_endpoint = IpEndpoint::new(
            match endpoint.addr.version() {
                IpVersion::Ipv4 => IpAddress::Ipv4(crate::wire::Ipv4Address::UNSPECIFIED),
                IpVersion::Ipv6 => IpAddress::Ipv6(crate::wire::Ipv6Address::UNSPECIFIED),
            },
            endpoint.port,
        );
        let mut range = tree.range((Included(unspecified_endpoint), Included(endpoint)));
        match range.next_back() {
            None => None,
            Some((&IpEndpoint { ref addr, .. }, h))
                if *addr == endpoint.addr || addr.is_unspecified() =>
            {
                Some(h)
            }
            _ => match range.next() {
                Some((e, h)) if e.addr.is_unspecified() => Some(h),
                _ => None,
            },
        }
    }
}
