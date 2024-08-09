/*! Network interface logic.

The `iface` module deals with the *network interfaces*. It filters incoming frames,
provides lookup and caching of hardware addresses, and handles management packets.
*/

mod fragmentation;
mod interface;
#[cfg(any(feature = "medium-ethernet", feature = "medium-ieee802154"))]
mod neighbor;
mod route;
#[cfg(feature = "proto-rpl")]
mod rpl;
#[cfg(feature = "std")]
mod socket_dispatch;
#[cfg(feature = "std")]
pub(crate) use self::socket_dispatch::DispatchTable;
#[cfg(not(feature = "std"))]
mod socket_dispatch_nostd;
#[cfg(not(feature = "std"))]
pub(crate) use self::socket_dispatch_nostd::DispatchTable;
mod socket_meta;
mod socket_set;

mod packet;

#[cfg(feature = "proto-igmp")]
pub use self::interface::MulticastError;
pub use self::interface::{Config, Interface, InterfaceInner as Context};

pub use self::route::{Route, RouteTableFull, Routes};
pub use self::socket_set::{SocketHandle, SocketSet, SocketStorage};
