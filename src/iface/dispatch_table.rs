use crate::wire::{IpEndpoint, IpProtocol, IpVersion};
#[cfg(feature = "std")]
use std::collections::{btree_map::Entry, BTreeMap, BTreeSet};

use super::SocketHandle;

#[derive(Debug)]
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

#[derive(Debug, Default)]
pub struct DispatchTable {
    raw: BTreeMap<(IpVersion, IpProtocol), SocketHandle>,
    udp: BTreeMap<IpEndpoint, SocketHandle>,
    tcp: BTreeMap<IpEndpoint, TcpLocalEndpoint>,

    rev_raw: BTreeMap<SocketHandle, (IpVersion, IpProtocol)>,
    rev_udp: BTreeMap<SocketHandle, IpEndpoint>,
    rev_tcp: BTreeMap<SocketHandle, (IpEndpoint, Option<IpEndpoint>)>,
}

#[derive(Debug)]
pub enum AddError {
    AlreadyInUse,
}

impl DispatchTable {
    pub(crate) fn add_socket(
        &mut self,
        upcast_socket: &crate::socket::Socket<'_>,
        handle: SocketHandle,
    ) -> Result<(), AddError> {
        match upcast_socket {
            crate::socket::Socket::Raw(socket) => {
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
            crate::socket::Socket::Icmp(_) => panic!("TODO"),
            crate::socket::Socket::Udp(socket) => {
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
            crate::socket::Socket::Tcp(socket) => {
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
                    .or_insert_with(TcpLocalEndpoint::new);

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
            crate::socket::Socket::Dhcpv4(_) => panic!("TODO"),
            crate::socket::Socket::Dns(_) => panic!("TODO"),
        }
    }
}

#[derive(Debug)]
pub enum RemoveError {
    SocketNotFound,
}

impl DispatchTable {
    pub(crate) fn remove_socket(
        &mut self,
        upcast_socket: &crate::socket::Socket<'_>,
        handle: SocketHandle,
    ) -> Result<(), RemoveError> {
        match *upcast_socket {
            crate::socket::Socket::Raw(_) => match self.rev_raw.entry(handle) {
                Entry::Vacant(_) => Err(RemoveError::SocketNotFound),
                Entry::Occupied(re) => match self.raw.entry(*re.get()) {
                    Entry::Vacant(_) => Err(RemoveError::SocketNotFound),
                    Entry::Occupied(e) => {
                        e.remove();
                        re.remove();
                        Ok(())
                    }
                },
            },
            crate::socket::Socket::Icmp(_) => panic!("TODO"),
            crate::socket::Socket::Udp(_) => match self.rev_udp.entry(handle) {
                Entry::Vacant(_) => Err(RemoveError::SocketNotFound),
                Entry::Occupied(re) => match self.udp.entry(*re.get()) {
                    Entry::Vacant(_) => Err(RemoveError::SocketNotFound),
                    Entry::Occupied(e) => {
                        e.remove();
                        re.remove();
                        Ok(())
                    }
                },
            },
            crate::socket::Socket::Tcp(_) => {
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
            crate::socket::Socket::Dhcpv4(_) => panic!("TODO"),
            crate::socket::Socket::Dns(_) => panic!("TODO"),
        }
    }
}

// tomtodo:
// functions that utilize the table to get according handle
// functions that modify the handle states (for socket tracker)
