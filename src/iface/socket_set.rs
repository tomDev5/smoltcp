use core::fmt;
use managed::ManagedSlice;

use super::{
    socket_meta::Meta,
    socket_tracker::{SocketTracker, TrackedSocket},
    DirtySockets, DispatchTable,
};
use crate::socket::{raw, tcp, udp, AnySocket, Socket};

/// Opaque struct with space for storing one socket.
///
/// This is public so you can use it to allocate space for storing
/// sockets when creating an Interface.
#[derive(Debug, Default)]
pub struct SocketStorage<'a> {
    inner: Option<Item<'a>>,
}

impl<'a> SocketStorage<'a> {
    pub const EMPTY: Self = Self { inner: None };
}

/// An item of a socket set.
#[derive(Debug)]
pub(crate) struct Item<'a> {
    pub(crate) meta: Meta,
    pub(crate) socket: Socket<'a>,
}

/// A handle, identifying a socket in an Interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SocketHandle(usize);

impl fmt::Display for SocketHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// An extensible set of sockets.
///
/// The lifetime `'a` is used when storing a `Socket<'a>`.  If you're using
/// owned buffers for your sockets (passed in as `Vec`s) you can use
/// `SocketSet<'static>`.
#[derive(Debug)]
pub struct SocketSet<'a> {
    sockets: ManagedSlice<'a, SocketStorage<'a>>,
    dispatch_table: DispatchTable,
    dirty_sockets: DirtySockets,
}

impl<'a> SocketSet<'a> {
    /// Create a socket set using the provided storage.
    pub fn new<SocketsT>(sockets: SocketsT) -> SocketSet<'a>
    where
        SocketsT: Into<ManagedSlice<'a, SocketStorage<'a>>>,
    {
        let sockets = sockets.into();
        SocketSet {
            sockets,
            dispatch_table: DispatchTable::default(),
            dirty_sockets: DirtySockets::default(),
        }
    }

    /// Add a socket to the set, and return its handle.
    ///
    /// # Panics
    /// This function panics if the storage is fixed-size (not a `Vec`) and is full.
    pub fn add<T: AnySocket<'a>>(&mut self, socket: T) -> SocketHandle {
        fn put<'a>(index: usize, slot: &mut SocketStorage<'a>, socket: Socket<'a>) -> SocketHandle {
            net_trace!("[{}]: adding", index);
            let handle = SocketHandle(index);
            let mut meta = Meta::default();
            meta.handle = handle;
            *slot = SocketStorage {
                inner: Some(Item { meta, socket }),
            };
            handle
        }

        let socket = socket.upcast();

        for (index, slot) in self.sockets.iter_mut().enumerate() {
            if slot.inner.is_none() {
                match &socket {
                    Socket::Raw(socket) => self
                        .dispatch_table
                        .add_raw_socket(&socket, SocketHandle(index))
                        .unwrap(),
                    Socket::Udp(socket) => self
                        .dispatch_table
                        .add_udp_socket(&socket, SocketHandle(index))
                        .unwrap(),
                    Socket::Tcp(socket) => self
                        .dispatch_table
                        .add_tcp_socket(&socket, SocketHandle(index))
                        .unwrap(), // todo not unwrap
                    _ => {}
                };
                return put(index, slot, socket);
            }
        }

        match &mut self.sockets {
            ManagedSlice::Borrowed(_) => panic!("adding a socket to a full SocketSet"),
            #[cfg(feature = "alloc")]
            ManagedSlice::Owned(sockets) => {
                sockets.push(SocketStorage { inner: None });
                let index = sockets.len() - 1;
                match &socket {
                    Socket::Raw(socket) => self
                        .dispatch_table
                        .add_raw_socket(&socket, SocketHandle(index))
                        .unwrap(),
                    Socket::Udp(socket) => self
                        .dispatch_table
                        .add_udp_socket(&socket, SocketHandle(index))
                        .unwrap(),
                    Socket::Tcp(socket) => self
                        .dispatch_table
                        .add_tcp_socket(&socket, SocketHandle(index))
                        .unwrap(), // todo not unwrap
                    _ => {}
                };
                put(index, &mut sockets[index], socket)
            }
        }
    }

    /// Get a socket from the set by its handle, as mutable.
    ///
    /// # Panics
    /// This function may panic if the handle does not belong to this socket set
    /// or the socket has the wrong type.
    pub fn get<T: AnySocket<'a>>(&self, handle: SocketHandle) -> &T {
        match self.sockets[handle.0].inner.as_ref() {
            Some(item) => {
                T::downcast(&item.socket).expect("handle refers to a socket of a wrong type")
            }
            None => panic!("handle does not refer to a valid socket"),
        }
    }

    /// Get a mutable socket from the set by its handle, as mutable.
    ///
    /// # Panics
    /// This function may panic if the handle does not belong to this socket set
    /// or the socket has the wrong type.
    pub fn get_mut<T: AnySocket<'a> + TrackedSocket>(
        &mut self,
        handle: SocketHandle,
    ) -> SocketTracker<T> {
        let Some(item) = self.sockets[handle.0].inner.as_mut() else {
            panic!("handle does not refer to a valid socket");
        };

        let socket =
            T::downcast_mut(&mut item.socket).expect("handle refers to a socket of a wrong type");

        SocketTracker::new(
            &mut self.dispatch_table,
            &mut self.dirty_sockets,
            handle,
            socket,
        )
    }

    /// Remove a socket from the set, without changing its state.
    ///
    /// # Panics
    /// This function may panic if the handle does not belong to this socket set.
    pub fn remove(&mut self, handle: SocketHandle) -> Socket<'a> {
        net_trace!("[{}]: removing", handle.0);
        let Some(item) = self.sockets[handle.0].inner.take() else {
            panic!("handle does not refer to a valid socket");
        };

        let socket = item.socket;

        match socket {
            Socket::Raw(_) => self.dispatch_table.remove_raw_socket(handle).unwrap(),
            Socket::Udp(_) => self.dispatch_table.remove_udp_socket(handle).unwrap(),
            Socket::Tcp(_) => self.dispatch_table.remove_tcp_socket(handle).unwrap(),
            _ => {}
        }

        socket
    }

    /// Get an iterator to the inner sockets.
    pub fn iter(&self) -> impl Iterator<Item = (SocketHandle, &Socket<'a>)> {
        self.items().map(|i| (i.meta.handle, &i.socket))
    }

    /// Get a mutable iterator to the inner sockets.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (SocketHandle, &mut Socket<'a>)> {
        self.items_mut().map(|i| (i.meta.handle, &mut i.socket))
    }

    /// Iterate every socket in this set.
    pub(crate) fn items(&self) -> impl Iterator<Item = &Item<'a>> + '_ {
        self.sockets.iter().filter_map(|x| x.inner.as_ref())
    }

    /// Iterate every socket in this set.
    pub(crate) fn items_mut(&mut self) -> impl Iterator<Item = &mut Item<'a>> + '_ {
        self.sockets.iter_mut().filter_map(|x| x.inner.as_mut())
    }

    pub(crate) fn get_mut_tcp_socket(
        &mut self,
        ip_repr: &crate::wire::IpRepr,
        tcp_repr: &crate::wire::TcpRepr,
    ) -> Option<SocketTracker<crate::socket::tcp::Socket<'a>>> {
        let handle: SocketHandle = self.dispatch_table.get_tcp_socket(ip_repr, tcp_repr)?;
        let socket = match self.sockets[handle.0].inner.as_mut() {
            Some(item) => tcp::Socket::downcast_mut(&mut item.socket)
                .expect("handle refers to a socket of a wrong type"),
            None => panic!("handle does not refer to a valid socket"),
        };
        let tracker = SocketTracker::new(
            &mut self.dispatch_table,
            &mut self.dirty_sockets,
            handle,
            socket,
        );
        Some(tracker)
    }

    pub(crate) fn get_mut_udp_socket(
        &mut self,
        ip_repr: &crate::wire::IpRepr,
        udp_repr: &crate::wire::UdpRepr,
    ) -> Option<SocketTracker<crate::socket::udp::Socket<'a>>> {
        let handle: SocketHandle = self.dispatch_table.get_udp_socket(ip_repr, udp_repr)?;

        let socket = match self.sockets[handle.0].inner.as_mut() {
            Some(item) => udp::Socket::downcast_mut(&mut item.socket)
                .expect("handle refers to a socket of a wrong type"),
            None => panic!("handle does not refer to a valid socket"),
        };
        let tracker = SocketTracker::new(
            &mut self.dispatch_table,
            &mut self.dirty_sockets,
            handle,
            socket,
        );
        Some(tracker)
    }

    pub(crate) fn get_mut_raw_socket(
        &mut self,
        ip_version: crate::wire::IpVersion,
        ip_protocol: crate::wire::IpProtocol,
    ) -> Option<SocketTracker<crate::socket::raw::Socket<'a>>> {
        let handle: SocketHandle = self
            .dispatch_table
            .get_raw_socket(ip_version, ip_protocol)?;

        let socket = match self.sockets[handle.0].inner.as_mut() {
            Some(item) => raw::Socket::downcast_mut(&mut item.socket)
                .expect("handle refers to a socket of a wrong type"),
            None => panic!("handle does not refer to a valid socket"),
        };
        let tracker = SocketTracker::new(
            &mut self.dispatch_table,
            &mut self.dirty_sockets,
            handle,
            socket,
        );
        Some(tracker)
    }
}
