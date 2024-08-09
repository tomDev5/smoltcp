pub struct DispatchTable {}

impl DispatchTable {
    pub(crate) fn get_tcp_socket(
        &self,
        ip_repr: &crate::wire::IpRepr,
        tcp_repr: &crate::wire::TcpRepr,
    ) -> Option<SocketHandle> {
        unimplemented!()
    }

    pub(crate) fn get_udp_socket(
        &self,
        ip_repr: &crate::wire::IpRepr,
        udp_repr: &crate::wire::UdpRepr,
    ) -> Option<SocketHandle> {
        unimplemented!()
    }

    pub(crate) fn get_raw_socket(
        &self,
        ip_version: crate::wire::IpVersion,
        ip_protocol: crate::wire::IpProtocol,
    ) -> Option<SocketHandle> {
        unimplemented!()
    }
}
