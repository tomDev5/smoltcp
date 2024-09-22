use managed::ManagedSlice;

use crate::{
    socket::{AnySocket, Socket},
    storage::RingBuffer,
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

use super::{
    dispatch::DispatchTable,
    socket_meta::Meta,
    socket_tracker::{SocketTracker, TrackedSocket},
    Context, SocketHandle, SocketSet, SocketStorage,
};

/// A container of sockets with packet dispathing.
#[derive(Debug)]
pub struct Container {
    set: SocketSet<'static>,
    dispatch_table: DispatchTable,
    dirty_sockets: RingBuffer<'static, SocketHandle>,
}

impl Container {
    pub fn new<SocketsT, DirtySocketsT>(
        sockets: SocketsT,
        dirty_sockets: DirtySocketsT,
    ) -> Container
    where
        SocketsT: Into<ManagedSlice<'static, SocketStorage<'static>>>,
        DirtySocketsT: Into<ManagedSlice<'static, SocketHandle>>,
    {
        Container {
            set: SocketSet::new(sockets),
            dispatch_table: DispatchTable::new(),
            dirty_sockets: RingBuffer::new_default(dirty_sockets),
        }
    }

    /// Add a generic socket to the set with the reference count 1, and return its handle.
    ///
    /// # Panics
    /// This function panics if the storage is fixed-size (not a `Vec`) and is full.
    pub fn add_upcast(
        &mut self,
        socket: Socket<'static>,
    ) -> Result<SocketHandle, super::dispatch::Error> {
        let handle = self.set.add_upcast(socket);
        while self.set.capacity() > self.dirty_sockets.capacity() {
            self.dirty_sockets.expand_storage();
        }
        self.dispatch_table
            .add_socket(self.set.get_upcast(handle), handle)?;
        Ok(handle)
    }

    /// Add a socket to the set with the reference count 1, and return its handle.
    ///
    /// # Panics
    /// This function panics if the storage is fixed-size (not a `Vec`) and is full.
    pub fn add<T: AnySocket<'static>>(
        &mut self,
        socket: T,
    ) -> Result<SocketHandle, super::dispatch::Error> {
        self.add_upcast(socket.upcast())
    }

    /// Get a tracked socket from the container by its handle, as mutable.
    ///
    /// # Panics
    /// This function may panic if the handle does not belong to this socket set.
    pub fn get_mut<'b, T: AnySocket<'static> + TrackedSocket>(
        &'b mut self,
        handle: SocketHandle,
    ) -> Option<SocketTracker<'b, T>> {
        if let Some(socket) = T::downcast_mut(self.set.get_mut_upcast(handle)) {
            Some(SocketTracker::new(
                &mut self.dispatch_table,
                &mut self.dirty_sockets,
                handle,
                socket,
            ))
        } else {
            None
        }
    }

    /// Remove a socket from the container, without changing its state.
    ///
    /// # Panics
    /// This function may panic if the handle does not belong to this socket set.
    pub fn remove(&mut self, handle: SocketHandle) -> Socket<'static> {
        let mut socket = self.set.remove(handle);
        let res = self.dispatch_table.remove_socket(&socket, handle);
        debug_assert!(res.is_ok());
        if socket.is_on_dirty_list() {
            let res = self.dirty_sockets.remove(&handle);
            socket.set_on_dirty_list(false);
            debug_assert!(res.is_ok());
        }
        socket
    }

    #[cfg(feature = "socket-raw")]
    pub(crate) fn get_raw_socket<'b>(
        &'b mut self,
        ip_repr: &IpRepr,
    ) -> Option<SocketTracker<'b, RawSocket<'static>>> {
        if let Some((raw_socket, handle)) =
            self.dispatch_table.get_raw_socket(&mut self.set, ip_repr)
        {
            Some(SocketTracker::new(
                &mut self.dispatch_table,
                &mut self.dirty_sockets,
                handle,
                raw_socket,
            ))
        } else {
            None
        }
    }

    #[cfg(feature = "socket-udp")]
    pub(crate) fn get_udp_socket<'b>(
        &'b mut self,
        cx: &mut Context,
        ip_repr: &IpRepr,
        udp_repr: &UdpRepr,
    ) -> Option<SocketTracker<'b, UdpSocket<'static>>> {
        if let Some((udp_socket, handle)) =
            self.dispatch_table
                .get_udp_socket(cx, &mut self.set, ip_repr, udp_repr)
        {
            Some(SocketTracker::new(
                &mut self.dispatch_table,
                &mut self.dirty_sockets,
                handle,
                udp_socket,
            ))
        } else {
            None
        }
    }

    #[cfg(feature = "socket-tcp")]
    pub(crate) fn get_tcp_socket<'b>(
        &'b mut self,
        cx: &mut Context,
        ip_repr: &IpRepr,
        tcp_repr: &TcpRepr,
    ) -> Option<SocketTracker<'b, TcpSocket<'static>>> {
        if let Some((tcp_socket, handle)) =
            self.dispatch_table
                .get_tcp_socket(cx, &mut self.set, ip_repr, tcp_repr)
        {
            Some(SocketTracker::new(
                &mut self.dispatch_table,
                &mut self.dirty_sockets,
                handle,
                tcp_socket,
            ))
        } else {
            None
        }
    }

    #[cfg(feature = "socket-dhcpv4")]
    pub(crate) fn get_dhcpv4_socket<'b>(
        &'b mut self,
        udp_repr: &UdpRepr,
    ) -> Option<SocketTracker<'b, Dhcpv4Socket<'static>>> {
        if let Some((dhcp_socket, handle)) = self
            .dispatch_table
            .get_dhcpv4_socket(&mut self.set, udp_repr)
        {
            Some(SocketTracker::new(
                &mut self.dispatch_table,
                &mut self.dirty_sockets,
                handle,
                dhcp_socket,
            ))
        } else {
            None
        }
    }

    #[cfg(feature = "socket-dhcpv4")]
    pub(crate) fn get_dns_socket<'b>(
        &'b mut self,
        ip_repr: &IpRepr,
        udp_repr: &UdpRepr,
    ) -> Option<SocketTracker<'b, DnsSocket<'static>>> {
        if let Some((dns_socket, handle)) =
            self.dispatch_table
                .get_dns_socket(&mut self.set, ip_repr, udp_repr)
        {
            Some(SocketTracker::new(
                &mut self.dispatch_table,
                &mut self.dirty_sockets,
                handle,
                dns_socket,
            ))
        } else {
            None
        }
    }

    #[cfg(all(feature = "socket-tcp", feature = "proto-ipv4"))]
    pub(crate) fn get_icmpv4_socket<'b>(
        &'b mut self,
        cx: &mut Context,
        ip_repr: &Ipv4Repr,
        icmp_repr: &Icmpv4Repr,
    ) -> Option<SocketTracker<'b, IcmpSocket<'static>>> {
        if let Some((icmpv4_socket, handle)) =
            self.dispatch_table
                .get_icmpv4_socket(cx, &mut self.set, ip_repr, icmp_repr)
        {
            Some(SocketTracker::new(
                &mut self.dispatch_table,
                &mut self.dirty_sockets,
                handle,
                icmpv4_socket,
            ))
        } else {
            None
        }
    }

    #[cfg(all(feature = "socket-tcp", feature = "proto-ipv6"))]
    pub(crate) fn get_icmpv6_socket<'b>(
        &'b mut self,
        cx: &mut Context,
        ip_repr: &Ipv6Repr,
        icmp_repr: &Icmpv6Repr,
    ) -> Option<SocketTracker<'b, IcmpSocket<'static>>> {
        if let Some((icmpv6_socket, handle)) =
            self.dispatch_table
                .get_icmpv6_socket(cx, &mut self.set, ip_repr, icmp_repr)
        {
            Some(SocketTracker::new(
                &mut self.dispatch_table,
                &mut self.dirty_sockets,
                handle,
                icmpv6_socket,
            ))
        } else {
            None
        }
    }

    pub(crate) fn next_dirty<'b>(
        &'b mut self,
    ) -> Option<(SocketTracker<'b, Socket<'static>>, &'b mut Meta)> {
        let handle = {
            match self.dirty_sockets.dequeue_one() {
                Err(_) => return None,
                Ok(handle) => *handle,
            }
        };
        let item = self.set.get_mut_item(handle);
        let socket = &mut item.socket;
        let meta = &mut item.meta;
        socket.set_on_dirty_list(false);
        Some((
            SocketTracker::new(
                &mut self.dispatch_table,
                &mut self.dirty_sockets,
                handle,
                socket,
            ),
            meta,
        ))
    }

    pub(crate) fn dirty_sockets_capacity(&self) -> usize {
        self.dirty_sockets.capacity()
    }

    // pub(crate) fn dirty_iter<'b>(&'b mut self) -> DirtyIter<'b> {
    //     let capacity = self.dirty_sockets.capacity();
    //     DirtyIter::new(self, capacity)
    // }
}

// // An iterator over dirty sockets with limited iteration count.
// // A socket is removed from the `dirty_sockets` list in `next_dirty`, but can be
// // added back in the `SocketTracker` destructor if it still has data to send.
// // To prevent infinite iteration we limit ourselves to `dirty_sockets.capacity`
// // by using `DirtyIter`.
// //
// // `DirtyIter` can't implement the `Iterator` trait because of non-standard lifetimes
// pub struct DirtyIter<'b> {
//     container: &'b mut Container,
//     sockets_left: usize,
// }

// impl<'b> DirtyIter<'b> {
//     pub fn new(container: &'b mut Container, sockets_left: usize) -> DirtyIter<'b> {
//         DirtyIter {
//             container,
//             sockets_left,
//         }
//     }

//     pub fn next<'c>(&'b mut self) -> Option<SocketTracker<'b, Socket<'static>>> {
//         if self.sockets_left == 0 {
//             None
//         } else {
//             self.sockets_left -= 1;
//             if let Some(tracker) = self.container.next_dirty() {
//                 Some(tracker)
//             } else {
//                 None
//             }
//         }
//     }
// }

#[cfg(test)]
mod test {
    // use crate::iface::SocketContainer;
    // use crate::socket::*;
    // use crate::wire::*;

    fn dispatcher() {
        //     let eps = [
        //         IpListenEndpoint::from(12345u16),
        //         IpListenEndpoint::from(IpEndpoint::new(
        //             IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 1)),
        //             12345u16,
        //         )),
        //         IpListenEndpoint::from(IpEndpoint::new(
        //             IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 2)),
        //             12345u16,
        //         )),
        //         IpListenEndpoint::from(IpEndpoint::new(
        //             IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 3)),
        //             12345u16,
        //         )),
        //         IpListenEndpoint::from(IpEndpoint::new(
        //             IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 4)),
        //             12345u16,
        //         )),
        //         IpListenEndpoint::from(IpEndpoint::new(
        //             IpAddress::Ipv4(Ipv4Address::new(192, 168, 1, 5)),
        //             12345u16,
        //         )),
        //     ];

        //     let mut sockets = SocketContainer::new(vec![], vec![]);

        //     let udp_rx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; 64])]);
        //     let udp_tx_buffer = UdpSocketBuffer::new(vec![UdpPacketBuffer::new(vec![0; 128])]);
        //     let udp_socket = UdpSocket::new(udp_rx_buffer, udp_tx_buffer);

        //     let tcp_rx_buffer = TcpSocketBuffer::new(vec![0; 64]);
        //     let tcp_tx_buffer = TcpSocketBuffer::new(vec![0; 128]);
        //     let tcp_socket = TcpSocket::new(tcp_rx_buffer, tcp_tx_buffer);

        //     let tcp_rx_buffer2 = TcpSocketBuffer::new(vec![0; 64]);
        //     let tcp_tx_buffer2 = TcpSocketBuffer::new(vec![0; 128]);
        //     let tcp_socket2 = TcpSocket::new(tcp_rx_buffer2, tcp_tx_buffer2);

        //     let tcp_rx_buffer3 = TcpSocketBuffer::new(vec![0; 64]);
        //     let tcp_tx_buffer3 = TcpSocketBuffer::new(vec![0; 128]);
        //     let tcp_socket3 = TcpSocket::new(tcp_rx_buffer3, tcp_tx_buffer3);

        //     let udp_handle = sockets.add(udp_socket).unwrap();
        //     let tcp_handle = sockets.add(tcp_socket).unwrap();
        //     let tcp_handle2 = sockets.add(tcp_socket2).unwrap();
        //     let tcp_handle3 = sockets.add(tcp_socket3).unwrap();

        //     {
        //         let mut tcp_socket = sockets.get_mut::<TcpSocket>(tcp_handle).unwrap();
        //         tcp_socket.set_debug_id(101);
        //         tcp_socket.listen(eps[0]).unwrap();
        //     }
        //     {
        //         let mut tcp_socket2 = sockets.get_mut::<TcpSocket>(tcp_handle2).unwrap();
        //         tcp_socket2.set_debug_id(102);
        //         tcp_socket2.listen(eps[2]).unwrap();
        //     }
        //     {
        //         let mut tcp_socket3 = sockets.get_mut::<TcpSocket>(tcp_handle3).unwrap();
        //         tcp_socket3.set_debug_id(103);
        //         tcp_socket3.listen(eps[4]).unwrap();
        //     }
        //     {
        //         let mut udp_socket = sockets.get_mut::<UdpSocket>(udp_handle).unwrap();
        //         udp_socket.set_debug_id(201);
        //         udp_socket.bind(eps[0]);
        //     }

        //     let tcp_payload = vec![];
        //     let udp_payload = vec![];

        //     let mut tcp_repr = TcpRepr {
        //         src_port: 9999u16,
        //         dst_port: 12345u16,
        //         control: TcpControl::Syn,
        //         push: false,
        //         seq_number: TcpSeqNumber(0),
        //         ack_number: None,
        //         window_len: 0u16,
        //         max_seg_size: None,
        //         payload: &tcp_payload,
        //     };

        //     let mut ipv4_repr = Ipv4Repr {
        //         src_addr: Ipv4Address::new(192, 168, 1, 100),
        //         dst_addr: Ipv4Address::new(192, 168, 1, 1),
        //         protocol: IpProtocol::Tcp,
        //         payload_len: 0,
        //     };

        //     let udp_repr = UdpRepr {
        //         src_port: 9999u16,
        //         dst_port: 12345u16,
        //         payload: &udp_payload,
        //     };

        //     {
        //         let tracked_socket = sockets
        //             .get_udp_socket(&IpRepr::Ipv4(ipv4_repr), &udp_repr)
        //             .unwrap();
        //         assert_eq!(tracked_socket.debug_id(), 201);
        //     }

        //     for &(ep_index, expected_debug_id) in &[(1, 101), (2, 102), (3, 101), (4, 103), (5, 101)] {
        //         ipv4_repr.dst_addr = match eps[ep_index].addr {
        //             IpAddress::Ipv4(a) => a,
        //             _ => unreachable!(),
        //         };
        //         tcp_repr.dst_port = eps[ep_index].port;
        //         let tracked_socket = sockets
        //             .get_tcp_socket(&IpRepr::Ipv4(ipv4_repr), &tcp_repr)
        //             .unwrap();
        //         assert_eq!(tracked_socket.debug_id(), expected_debug_id);
        //     }

        //     sockets.remove(udp_handle);

        //     assert!(sockets
        //         .get_udp_socket(&IpRepr::Ipv4(ipv4_repr), &udp_repr)
        //         .is_none());

        //     ipv4_repr.dst_addr = Ipv4Address::new(192, 168, 1, 2);

        //     assert_eq!(
        //         sockets
        //             .get_tcp_socket(&IpRepr::Ipv4(ipv4_repr), &tcp_repr)
        //             .unwrap()
        //             .debug_id(),
        //         102
        //     );

        //     sockets.remove(tcp_handle2);

        //     assert_eq!(
        //         sockets
        //             .get_tcp_socket(&IpRepr::Ipv4(ipv4_repr), &tcp_repr)
        //             .unwrap()
        //             .debug_id(),
        //         101
        //     );
    }
}
