use sockfw::*;
use std::net::SocketAddr;

fn main() {
    let listener = tcp::TcpListener::bind(
        &"0.0.0.0:8443".parse::<SocketAddr>().unwrap()
    ).unwrap();

    let connector = unix::UnixConnector::new(
        "/var/run/docker.sock"
    );

    Fw::new(
        listener,
        connector,
        1024,
        2048,
        4096
    ).unwrap().run();
}
