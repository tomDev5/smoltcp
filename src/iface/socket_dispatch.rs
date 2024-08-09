use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::Included;

use crate::socket::{raw, tcp, udp};
use crate::wire::{IpAddress, IpEndpoint, IpProtocol, IpVersion};

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

#[derive(Debug)]
pub enum AddError {
    AlreadyInUse,
}

impl DispatchTable {
    pub(crate) fn add_raw_socket(
        &mut self,
        socket: &raw::Socket<'_>,
        handle: SocketHandle,
    ) -> Result<(), AddError> {
        let key = (socket.ip_version(), socket.ip_protocol());
        match (self.raw.entry(key), self.rev_raw.entry(handle)) {
            (Entry::Vacant(e), Entry::Vacant(re)) => {
                e.insert(handle);
                re.insert(key);
            }
            _ => return Err(AddError::AlreadyInUse),
        };
        Ok(())
    }

    pub(crate) fn add_udp_socket(
        &mut self,
        socket: &udp::Socket<'_>,
        handle: SocketHandle,
    ) -> Result<(), AddError> {
        if !socket.endpoint().is_specified() && socket.endpoint().port == 0 {
            return Ok(());
        }
        let endpoint = socket.endpoint();
        let Some(endpoint) = endpoint
            .addr
            .map(|addr| IpEndpoint::new(addr, endpoint.port))
        else {
            return Ok(());
        };

        match (self.udp.entry(endpoint), self.rev_udp.entry(handle)) {
            (Entry::Vacant(e), Entry::Vacant(re)) => {
                e.insert(handle);
                re.insert(endpoint);
            }
            _ => return Err(AddError::AlreadyInUse),
        };
        Ok(())
    }

    pub(crate) fn add_tcp_socket(
        &mut self,
        socket: &tcp::Socket<'_>,
        handle: SocketHandle,
    ) -> Result<(), AddError> {
        let Some(local_endpoint) = socket.local_endpoint() else {
            return Ok(());
        };

        let rev_entry = match self.rev_tcp.entry(handle) {
            Entry::Occupied(_) => return Err(AddError::AlreadyInUse),
            Entry::Vacant(e) => e,
        };

        let tcp_endpoint = self
            .tcp
            .entry(local_endpoint)
            .or_insert_with(TcpLocalEndpoint::default);

        if let Some(remote_endpoint) = socket.remote_endpoint() {
            // socket established
            match tcp_endpoint.established_sockets.entry(remote_endpoint) {
                Entry::Occupied(_) => return Err(AddError::AlreadyInUse),
                Entry::Vacant(e) => {
                    e.insert(handle);
                    rev_entry.insert((local_endpoint, Some(remote_endpoint)));
                }
            };
        } else {
            // socket not established
            tcp_endpoint.listen_sockets.insert(handle);
            rev_entry.insert((local_endpoint, None));
        }

        Ok(())
    }
}

#[derive(Debug)]
pub enum RemoveError {
    SocketNotFound,
}

impl DispatchTable {
    pub(crate) fn remove_raw_socket(&mut self, handle: SocketHandle) -> Result<(), RemoveError> {
        match self.rev_raw.entry(handle) {
            Entry::Vacant(_) => Err(RemoveError::SocketNotFound),
            Entry::Occupied(re) => match self.raw.entry(*re.get()) {
                Entry::Vacant(_) => Err(RemoveError::SocketNotFound),
                Entry::Occupied(e) => {
                    e.remove();
                    re.remove();
                    Ok(())
                }
            },
        }
    }

    pub(crate) fn remove_udp_socket(&mut self, handle: SocketHandle) -> Result<(), RemoveError> {
        match self.rev_udp.entry(handle) {
            Entry::Vacant(_) => Err(RemoveError::SocketNotFound),
            Entry::Occupied(re) => match self.udp.entry(*re.get()) {
                Entry::Vacant(_) => Err(RemoveError::SocketNotFound),
                Entry::Occupied(e) => {
                    e.remove();
                    re.remove();
                    Ok(())
                }
            },
        }
    }

    pub(crate) fn remove_tcp_socket(&mut self, handle: SocketHandle) -> Result<(), RemoveError> {
        let re = match self.rev_tcp.entry(handle) {
            Entry::Vacant(_) => return Err(RemoveError::SocketNotFound),
            Entry::Occupied(re) => re,
        };

        let &(local_endpoint, remote_endpoint) = re.get();

        let mut tc_endpoint_entry = match self.tcp.entry(local_endpoint) {
            Entry::Vacant(_) => return Err(RemoveError::SocketNotFound),
            Entry::Occupied(e) => e,
        };

        {
            let tcp_endpoint = tc_endpoint_entry.get_mut();

            if let Some(remote_endpoint) = remote_endpoint {
                // socket bound
                match tcp_endpoint.established_sockets.entry(remote_endpoint) {
                    Entry::Occupied(o) => {
                        o.remove();
                        re.remove();
                    }
                    Entry::Vacant(_) => return Err(RemoveError::SocketNotFound),
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
}
