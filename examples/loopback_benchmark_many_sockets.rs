mod utils;
use log::debug;
use smoltcp::iface::{Config, Interface, SocketContainer};
use smoltcp::phy::{Device, Loopback, Medium};
use smoltcp::socket::tcp;
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr};
const NUM_PAIRS: u16 = 256;
fn main() {
    let device = Loopback::new(Medium::Ethernet);
    let mut device = {
        utils::setup_logging("info");
        let (mut opts, mut free) = utils::create_options();
        utils::add_middleware_options(&mut opts, &mut free);
        let mut matches = utils::parse_options(&opts, free);
        utils::parse_middleware_options(&mut matches, device, /*loopback=*/ true)
    };
    // Create interface
    let config = match device.capabilities().medium {
        Medium::Ethernet => {
            Config::new(EthernetAddress([0x02, 0x00, 0x00, 0x00, 0x00, 0x01]).into())
        }
        Medium::Ip => Config::new(smoltcp::wire::HardwareAddress::Ip),
        Medium::Ieee802154 => todo!(),
    };
    let mut iface = Interface::new(config, &mut device, Instant::now());
    iface.update_ip_addrs(|ip_addrs| {
        ip_addrs
            .push(IpCidr::new(IpAddress::v4(127, 0, 0, 1), 8))
            .unwrap();
    });
    // Create sockets
    let mut sockets = SocketContainer::new(Vec::new(), Vec::new());
    let mut server_handles = Vec::new();
    let mut client_handles = Vec::new();
    for client_port in 0..NUM_PAIRS {
        let server_socket = {
            let tcp_rx_buffer = tcp::SocketBuffer::new(vec![0; 65536]);
            let tcp_tx_buffer = tcp::SocketBuffer::new(vec![0; 65536]);
            let mut socket = tcp::Socket::new(tcp_rx_buffer, tcp_tx_buffer);
            socket.listen(1234).unwrap();
            socket
        };
        let client_socket = {
            let tcp_rx_buffer = tcp::SocketBuffer::new(vec![0; 65536]);
            let tcp_tx_buffer = tcp::SocketBuffer::new(vec![0; 65536]);
            let mut socket = tcp::Socket::new(tcp_rx_buffer, tcp_tx_buffer);
            let cx = iface.context();
            socket
                .connect(cx, (IpAddress::v4(127, 0, 0, 1), 1234), client_port + 1)
                .unwrap();
            socket
        };
        server_handles.push(sockets.add(server_socket).unwrap());
        client_handles.push(sockets.add(client_socket).unwrap());
    }
    let start_time = Instant::now();
    let mut processed = 0;
    while processed < 1024 * 1024 * 1024 {
        iface.poll(Instant::now(), &mut device, &mut sockets);
        for server_handle in &server_handles {
            let mut socket = sockets.get_mut::<tcp::Socket>(*server_handle).unwrap();
            while socket.can_recv() {
                let received = socket.recv(|buffer| (buffer.len(), buffer.len())).unwrap();
                debug!("got {:?}", received,);
                processed += received;
            }
        }
        for client_handle in &client_handles {
            let mut socket = sockets.get_mut::<tcp::Socket>(*client_handle).unwrap();
            while socket.can_send() {
                debug!("sending");
                socket.send(|buffer| (buffer.len(), ())).unwrap();
            }
        }
    }
    let duration = Instant::now() - start_time;
    println!(
        "done in {} s, bandwidth is {} Gbps",
        duration.total_millis() as f64 / 1000.0,
        (processed as u64 * 8 / duration.total_millis()) as f64 / 1000000.0
    );
}
