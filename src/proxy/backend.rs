use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct Backend {
    pub name: String,
    pub listen_addr: SocketAddr,
}

impl Backend {
    pub fn new(name: String, listen_addr: SocketAddr) -> Backend {
        Backend { name, listen_addr }
    }
}

// fn tcp_health_check(server: SocketAddr) -> bool {
//     if let Ok(_) = TcpStream::connect_timeout(&server, time::Duration::from_secs(3)) {
//         true
//     } else {
//         false
//     }
// }
