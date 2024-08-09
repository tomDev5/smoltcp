use std::{
    collections::BTreeSet,
    ops::{Deref, DerefMut},
};

use crate::{
    socket::{raw, tcp, udp},
    storage::RingBuffer,
    wire::{IpEndpoint, IpListenEndpoint},
};

use super::{DispatchTable, SocketHandle};

/// A trait for socket-type-depending tracking logic.
///
/// Used for upkeep of dispatching tables.
pub trait TrackedSocket {
    type State;

    fn new_state(&self) -> Self::State;
    fn on_drop(
        &mut self,
        _state: &Self::State,
        _dispatch_table: &mut DispatchTable,
        _handle: SocketHandle,
    ) {
    }

    fn is_dirty(&self) -> bool;
    fn is_on_dirty_list(&self) -> bool;
    fn set_on_dirty_list(&mut self, is_dirty: bool);
}

impl<'a> TrackedSocket for raw::Socket<'a> {
    type State = ();

    fn new_state(&self) -> Self::State {
        ()
    }

    fn is_dirty(&self) -> bool {
        self.is_dirty()
    }

    fn is_on_dirty_list(&self) -> bool {
        self.is_on_dirty_list()
    }

    fn set_on_dirty_list(&mut self, is_dirty: bool) {
        self.set_on_dirty_list(is_dirty)
    }
}

impl<'a> TrackedSocket for udp::Socket<'a> {
    type State = IpListenEndpoint;

    fn new_state(&self) -> Self::State {
        self.endpoint()
    }

    fn on_drop(
        &mut self,
        &old_endpoint: &Self::State,
        dispatch_table: &mut DispatchTable,
        handle: SocketHandle,
    ) {
        if old_endpoint != self.endpoint() {
            if old_endpoint.is_specified() {
                let res = dispatch_table.remove_udp_socket(handle);
                debug_assert!(res.is_ok());
            }
            let res = dispatch_table.add_udp_socket(self, handle);
            debug_assert!(res.is_ok());
        }
    }

    fn is_dirty(&self) -> bool {
        self.is_dirty()
    }

    fn is_on_dirty_list(&self) -> bool {
        self.is_on_dirty_list()
    }

    fn set_on_dirty_list(&mut self, is_dirty: bool) {
        self.set_on_dirty_list(is_dirty)
    }
}

impl<'a> TrackedSocket for tcp::Socket<'a> {
    type State = tcp::State;

    fn new_state(&self) -> Self::State {
        self.state()
    }

    fn on_drop(
        &mut self,
        state: &Self::State,
        dispatch_table: &mut DispatchTable,
        handle: SocketHandle,
    ) {
        if state == &self.state() {
            return;
        }

        match (state, self.state()) {
            (_, tcp::State::Closed) => {
                let res = dispatch_table.remove_tcp_socket(handle);
                debug_assert!(res.is_ok());
            }
            (tcp::State::Closed, _) => {
                let res = dispatch_table.add_tcp_socket(self, handle);
                debug_assert!(res.is_ok());
            }
            (tcp::State::TimeWait, _) | (tcp::State::Listen, _) => {
                let res = dispatch_table.remove_tcp_socket(handle);
                debug_assert!(res.is_ok());
                let res = dispatch_table.add_tcp_socket(self, handle);
                debug_assert!(res.is_ok());
            }
            (_, _) => {}
        }
    }

    fn is_dirty(&self) -> bool {
        self.is_dirty()
    }

    fn is_on_dirty_list(&self) -> bool {
        self.is_on_dirty_list()
    }

    fn set_on_dirty_list(&mut self, is_dirty: bool) {
        self.set_on_dirty_list(is_dirty)
    }
}

/// A tracking smart-pointer to a socket.
///
/// Implements `Deref` and `DerefMut` to the socket it contains.
/// Keeps the dispatching tables up to date by updating them in `drop`.
#[derive(Debug)]
pub struct SocketTracker<'a, T: TrackedSocket + 'a> {
    handle: SocketHandle,
    socket: &'a mut T,
    dispatch_table: &'a mut DispatchTable,
    dirty_sockets: &'a mut BTreeSet<SocketHandle>,
    state: T::State,
}

impl<'a, T: TrackedSocket + 'a> SocketTracker<'a, T> {
    pub(crate) fn new(
        dispatch_table: &'a mut DispatchTable,
        dirty_sockets: &'a mut BTreeSet<SocketHandle>,
        handle: SocketHandle,
        socket: &'a mut T,
    ) -> Self {
        let state = TrackedSocket::new_state(socket);
        SocketTracker {
            handle,
            dispatch_table,
            socket,
            state,
            dirty_sockets,
        }
    }
}

pub enum SocketState<'a> {
    Raw(<raw::Socket<'a> as TrackedSocket>::State),
    Udp(<udp::Socket<'a> as TrackedSocket>::State),
    Tcp(<tcp::Socket<'a> as TrackedSocket>::State),
}

impl<'a> TrackedSocket for crate::socket::Socket<'a> {
    type State = SocketState<'a>;

    fn new_state(&self) -> Self::State {
        match self {
            crate::socket::Socket::Raw(socket) => SocketState::Raw(socket.new_state()),
            crate::socket::Socket::Icmp(_) => unreachable!(),
            crate::socket::Socket::Udp(socket) => SocketState::Udp(socket.new_state()),
            crate::socket::Socket::Tcp(socket) => SocketState::Tcp(socket.new_state()),
            crate::socket::Socket::Dhcpv4(_) => unreachable!(),
            crate::socket::Socket::Dns(_) => unreachable!(),
        }
    }

    fn on_drop(
        &mut self,
        state: &Self::State,
        dispatch_table: &mut DispatchTable,
        handle: SocketHandle,
    ) {
        match (state, self) {
            (SocketState::Raw(state), crate::socket::Socket::Raw(socket)) => {
                socket.on_drop(state, dispatch_table, handle)
            }
            (SocketState::Udp(state), crate::socket::Socket::Udp(socket)) => {
                socket.on_drop(state, dispatch_table, handle)
            }
            (SocketState::Tcp(state), crate::socket::Socket::Tcp(socket)) => {
                socket.on_drop(state, dispatch_table, handle)
            }
            _ => unreachable!(),
        }
    }

    fn is_dirty(&self) -> bool {
        match self {
            crate::socket::Socket::Raw(socket) => socket.is_dirty(),
            crate::socket::Socket::Udp(socket) => socket.is_dirty(),
            crate::socket::Socket::Tcp(socket) => socket.is_dirty(),
            _ => unreachable!(),
        }
    }

    fn is_on_dirty_list(&self) -> bool {
        match self {
            crate::socket::Socket::Raw(socket) => socket.is_on_dirty_list(),
            crate::socket::Socket::Udp(socket) => socket.is_on_dirty_list(),
            crate::socket::Socket::Tcp(socket) => socket.is_on_dirty_list(),
            _ => unreachable!(),
        }
    }

    fn set_on_dirty_list(&mut self, is_dirty: bool) {
        match self {
            crate::socket::Socket::Raw(socket) => socket.set_on_dirty_list(is_dirty),
            crate::socket::Socket::Udp(socket) => socket.set_on_dirty_list(is_dirty),
            crate::socket::Socket::Tcp(socket) => socket.set_on_dirty_list(is_dirty),
            _ => unreachable!(),
        }
    }
}

impl<'a, T: TrackedSocket + 'a> Drop for SocketTracker<'a, T> {
    fn drop(&mut self) {
        self.socket
            .on_drop(&self.state, self.dispatch_table, self.handle);
        if !TrackedSocket::is_on_dirty_list(self.socket) && TrackedSocket::is_dirty(self.socket) {
            self.dirty_sockets.insert(self.handle);
        }
    }
}

impl<'a, T: TrackedSocket + 'a> Deref for SocketTracker<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.socket
    }
}

impl<'a, T: TrackedSocket + 'a> DerefMut for SocketTracker<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.socket
    }
}
