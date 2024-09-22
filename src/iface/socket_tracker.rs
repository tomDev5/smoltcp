use core::fmt::Debug;
use std::ops::{Deref, DerefMut};

use crate::{storage::RingBuffer, wire::IpListenEndpoint};

#[cfg(feature = "socket-udp")]
use crate::socket::udp::Socket as UdpSocket;

#[cfg(feature = "socket-raw")]
use crate::socket::raw::Socket as RawSocket;

#[cfg(feature = "socket-tcp")]
use crate::socket::tcp::Socket as TcpSocket;

#[cfg(feature = "socket-dhcpv4")]
use crate::socket::dhcpv4::Socket as Dhcpv4Socket;

#[cfg(feature = "socket-dns")]
use crate::socket::dns::Socket as DnsSocket;

#[cfg(feature = "socket-icmp")]
use crate::socket::icmp::Socket as IcmpSocket;

use super::{dispatch::DispatchTable, SocketHandle};

/// A trait for socket-type-depending tracking logic.
///
/// Used for upkeep of dispatching tables.
pub trait TrackedSocket {
    type State: Debug;

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

#[cfg(feature = "socket-raw")]
impl<'a> TrackedSocket for RawSocket<'a> {
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

#[cfg(feature = "socket-udp")]
impl<'a> TrackedSocket for UdpSocket<'a> {
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

#[cfg(feature = "socket-tcp")]
impl<'a> TrackedSocket for TcpSocket<'a> {
    type State = crate::socket::tcp::State;

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
            (_, crate::socket::tcp::State::Closed) => {
                let res = dispatch_table.remove_tcp_socket(handle);
                debug_assert!(res.is_ok());
            }
            (crate::socket::tcp::State::Closed, _) => {
                let res = dispatch_table.add_tcp_socket(self, handle);
                debug_assert!(res.is_ok());
            }
            (crate::socket::tcp::State::TimeWait, _) | (crate::socket::tcp::State::Listen, _) => {
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

/// These sockets do not yet have dispatch tables, TrackedSocket implementation is empty

impl<'a> TrackedSocket for IcmpSocket<'a> {
    type State = ();

    fn new_state(&self) -> Self::State {
        ()
    }

    fn is_dirty(&self) -> bool {
        false
    }

    fn is_on_dirty_list(&self) -> bool {
        false
    }

    fn set_on_dirty_list(&mut self, _is_dirty: bool) {}
}

impl<'a> TrackedSocket for Dhcpv4Socket<'a> {
    type State = ();

    fn new_state(&self) -> Self::State {
        ()
    }

    fn is_dirty(&self) -> bool {
        false
    }

    fn is_on_dirty_list(&self) -> bool {
        false
    }

    fn set_on_dirty_list(&mut self, _is_dirty: bool) {}
}

impl<'a> TrackedSocket for DnsSocket<'a> {
    type State = ();

    fn new_state(&self) -> Self::State {
        ()
    }

    fn is_dirty(&self) -> bool {
        false
    }

    fn is_on_dirty_list(&self) -> bool {
        false
    }

    fn set_on_dirty_list(&mut self, _is_dirty: bool) {}
}

/// A tracking smart-pointer to a socket.
///
/// Implements `Deref` and `DerefMut` to the socket it contains.
/// Keeps the dispatching tables up to date by updating them in `drop`.
#[derive(Debug)]
pub struct SocketTracker<'b, T: TrackedSocket> {
    handle: SocketHandle,
    socket: &'b mut T,
    dispatch_table: &'b mut DispatchTable,
    dirty_sockets: &'b mut RingBuffer<'static, SocketHandle>,
    state: T::State,
}

impl<'b, T: TrackedSocket> SocketTracker<'b, T> {
    pub(crate) fn new(
        dispatch_table: &'b mut DispatchTable,
        dirty_sockets: &'b mut RingBuffer<'static, SocketHandle>,
        handle: SocketHandle,
        socket: &'b mut T,
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

#[derive(Debug)]
pub enum SocketState<'a> {
    Raw(<RawSocket<'a> as TrackedSocket>::State),
    Udp(<UdpSocket<'a> as TrackedSocket>::State),
    Tcp(<TcpSocket<'a> as TrackedSocket>::State),
}

impl<'a> TrackedSocket for crate::socket::Socket<'a> {
    type State = SocketState<'a>;

    fn new_state(&self) -> Self::State {
        match self {
            crate::socket::Socket::Raw(socket) => SocketState::Raw(socket.new_state()),
            crate::socket::Socket::Udp(socket) => SocketState::Udp(socket.new_state()),
            crate::socket::Socket::Tcp(socket) => SocketState::Tcp(socket.new_state()),
            _ => unreachable!(),
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

impl<'b, T: TrackedSocket> Drop for SocketTracker<'b, T> {
    fn drop(&mut self) {
        net_trace!(
            "dropping tracked socket - old state: {:?}, new state: {:?}",
            self.state,
            self.socket.new_state()
        );
        self.socket
            .on_drop(&self.state, self.dispatch_table, self.handle);
        if !TrackedSocket::is_on_dirty_list(self.socket) && TrackedSocket::is_dirty(self.socket) {
            match self.dirty_sockets.enqueue_one() {
                Ok(h) => {
                    *h = self.handle;
                    TrackedSocket::set_on_dirty_list(self.socket, true);
                }
                _ => {
                    unreachable!();
                }
            }
        }
    }
}

impl<'b, T: TrackedSocket> Deref for SocketTracker<'b, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.socket
    }
}

impl<'b, T: TrackedSocket> DerefMut for SocketTracker<'b, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.socket
    }
}
