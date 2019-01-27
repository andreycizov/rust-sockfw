use sockfw::*;
use std::net::SocketAddr;
use openssl::ssl::{SslMethod, SslAcceptor};
use std::fs::OpenOptions;

fn main() {
//    let listener = tcp::TcpListener::bind(
//        &"0.0.0.0:8443".parse::<SocketAddr>().unwrap()
//    ).unwrap();

    let open_read = OpenOptions::new().read(true).clone();

    let mut acceptor = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
    acceptor.set_certificate(&ssl::SslListener::cert_from_file(&mut open_read.open("./etc/server.crt").unwrap()).unwrap()).unwrap();
    acceptor.set_private_key(&ssl::SslListener::pkey_from_file(&mut open_read.open("./etc/server.pem").unwrap()).unwrap()).unwrap();
    acceptor.set_ca_file("./etc/ca.crt").unwrap();
    acceptor.check_private_key().unwrap();

    let acceptor = acceptor.build();

    let listener = ssl::SslListener::bind(
        &"0.0.0.0:8443".parse::<SocketAddr>().unwrap(),
        acceptor
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
