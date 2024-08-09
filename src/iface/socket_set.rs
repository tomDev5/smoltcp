use core::fmt;
use managed::ManagedSlice;

use super::{
    dispatch_table::DispatchTable,
    socket_meta::Meta,
    socket_tracker::{SocketTracker, TrackedSocket},
};
#[cfg(any(feature = "std"))]
use crate::iface::dispatch_table;
use crate::{
    socket::{raw, tcp, udp, AnySocket, Socket},
    wire::{IpEndpoint, IpProtocol, IpVersion},
};
#[cfg(feature = "std")]
use std::collections::BTreeSet;

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
    #[cfg(any(feature = "std"))]
    dispatch_table: DispatchTable,
    #[cfg(any(feature = "std"))]
    dirty_sockets: BTreeSet<SocketHandle>,
}

#[derive(Debug)]
pub enum Error {
    SocketEndpointAlreadyInUse,
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
            #[cfg(any(feature = "std"))]
            dispatch_table: DispatchTable::default(),
            #[cfg(any(feature = "std"))]
            dirty_sockets: BTreeSet::default(),
        }
    }

    /// Add a socket to the set, and return its handle.
    ///
    /// # Panics
    /// This function panics if the storage is fixed-size (not a `Vec`) and is full.
    pub fn add<T: AnySocket<'a>>(&mut self, socket: T) -> Result<SocketHandle, Error> {
        let mut put = |index: usize,
                       slot: &mut SocketStorage<'a>,
                       socket: Socket<'a>|
         -> Result<SocketHandle, Error> {
            net_trace!("[{}]: adding", index);
            let handle = SocketHandle(index);
            let mut meta = Meta::default();
            meta.handle = handle;

            #[cfg(any(feature = "std"))]
            match self.dispatch_table.add_socket(&socket, handle) {
                Ok(()) => {
                    *slot = SocketStorage {
                        inner: Some(Item { meta, socket }),
                    };

                    Ok(handle)
                }
                Err(dispatch_table::AddError::AlreadyInUse) => {
                    return Err(Error::SocketEndpointAlreadyInUse)
                }
            }
        };

        let socket = socket.upcast();

        for (index, slot) in self.sockets.iter_mut().enumerate() {
            if slot.inner.is_none() {
                return put(index, slot, socket);
            }
        }

        match &mut self.sockets {
            ManagedSlice::Borrowed(_) => panic!("adding a socket to a full SocketSet"),
            #[cfg(feature = "alloc")]
            ManagedSlice::Owned(sockets) => {
                sockets.push(SocketStorage { inner: None });
                let index = sockets.len() - 1;
                match put(index, &mut sockets[index], socket) {
                    Ok(handle) => Ok(handle),
                    Err(Error::SocketEndpointAlreadyInUse) => {
                        sockets.pop();
                        Err(Error::SocketEndpointAlreadyInUse)
                    }
                }
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
    pub fn get_mut<T: AnySocket<'a>>(&mut self, handle: SocketHandle) -> &mut T {
        match self.sockets[handle.0].inner.as_mut() {
            Some(item) => T::downcast_mut(&mut item.socket)
                .expect("handle refers to a socket of a wrong type"),
            None => panic!("handle does not refer to a valid socket"),
        }
    }

    /// Get a mutable socket from the set by its handle, as mutable.
    ///
    /// # Panics
    /// This function may panic if the handle does not belong to this socket set
    /// or the socket has the wrong type.
    pub fn get_mut_tracked<T: AnySocket<'a> + TrackedSocket>(
        &mut self,
        handle: SocketHandle,
    ) -> SocketTracker<T> {
        match self.sockets[handle.0].inner.as_mut() {
            Some(item) => SocketTracker::new(
                &mut self.dispatch_table,
                &mut self.dirty_sockets,
                handle,
                T::downcast_mut(&mut item.socket)
                    .expect("handle refers to a socket of a wrong type"),
            ),
            None => panic!("handle does not refer to a valid socket"),
        }
    }

    /// Remove a socket from the set, without changing its state.
    ///
    /// # Panics
    /// This function may panic if the handle does not belong to this socket set.
    pub fn remove(&mut self, handle: SocketHandle) -> Socket<'a> {
        net_trace!("[{}]: removing", handle.0);
        let socket = match self.sockets[handle.0].inner.take() {
            Some(item) => item.socket,
            None => panic!("handle does not refer to a valid socket"),
        };

        #[cfg(any(feature = "std"))]
        match self.dispatch_table.remove_socket(&socket, handle) {
            Ok(()) => {
                self.dirty_sockets.remove(&handle);
            }
            Err(dispatch_table::RemoveError::SocketNotFound) => {
                panic!("handle does not refer to a valid dispatch item")
            }
        }

        socket
    }

    pub(crate) fn get_raw_socket(
        &'a mut self,
        ip_version: IpVersion,
        ip_protocol: IpProtocol,
    ) -> Option<SocketTracker<'a, raw::Socket<'a>>> {
        let handle = self
            .dispatch_table
            .get_raw_socket(ip_version, ip_protocol)?;

        Some(self.get_mut_tracked::<raw::Socket>(handle))
    }

    pub(crate) fn get_udp_socket(
        &'a mut self,
        endpoint: IpEndpoint,
    ) -> Option<SocketTracker<'a, udp::Socket<'a>>> {
        let handle = self.dispatch_table.get_udp_socket(endpoint)?;

        Some(self.get_mut_tracked::<udp::Socket>(handle))
    }

    pub(crate) fn get_tcp_socket(
        &'a mut self,
        local_endpoint: IpEndpoint,
        remote_endpoint: Option<IpEndpoint>,
    ) -> Option<SocketTracker<'a, tcp::Socket<'a>>> {
        let handle = self
            .dispatch_table
            .get_tcp_socket(local_endpoint, remote_endpoint)?;

        Some(self.get_mut_tracked::<tcp::Socket>(handle))
    }

    fn next_dirty<'b, T: AnySocket<'a> + TrackedSocket>(
        &'b mut self,
    ) -> Option<SocketTracker<'b, T>>
    where
        'a: 'b,
    {
        let handle = self.dirty_sockets.pop_first()?;
        let socket = match self.sockets[handle.0].inner.as_mut() {
            Some(item) => T::downcast_mut(&mut item.socket)
                .expect("handle refers to a socket of a wrong type"),
            None => panic!("handle does not refer to a valid socket"),
        };
        socket.set_on_dirty_list(false);
        Some(SocketTracker::new(
            &mut self.dispatch_table,
            &mut self.dirty_sockets,
            handle,
            socket,
        ))
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
}
