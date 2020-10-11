use std::net::UdpSocket;

pub fn find_available_port() -> Result<u16, std::io::Error> {
    Ok(UdpSocket::bind("127.0.0.1:0")?.local_addr()?.port())
}
