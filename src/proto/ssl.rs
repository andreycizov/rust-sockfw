use std::io::{Error as IoError, ErrorKind, Read, Write};
use std::net::SocketAddr;
use openssl::ssl::{HandshakeError, MidHandshakeSslStream, SslStream, SslAcceptor, SslMethod, SslFiletype};
use openssl::error::{Error as OrigSslError, ErrorStack};
use openssl::pkey::{PKey, Private};
use openssl::x509::X509;
use mio::tcp::{TcpListener as MioTcpListener, TcpStream};
use mio::{Poll, Token, Ready, PollOpt};
use clap::{App, Arg, ArgMatches};

use crate::{Listener, MidChan, Chan, FwError, Pollable, NextState};
use crate::args::Parsable;
use crate::proto::common::StreamConf;


#[derive(Debug)]
pub enum SslError {
    Io(IoError),
    Ssl(OrigSslError),
    SslStack(ErrorStack),
    Handshake(HandshakeError<TcpStream>),
    Str(String),
}

impl From<OrigSslError> for SslError {
    fn from(x: OrigSslError) -> Self {
        SslError::Ssl(x)
    }
}

impl From<ErrorStack> for SslError {
    fn from(x: ErrorStack) -> Self {
        SslError::SslStack(x)
    }
}


impl From<IoError> for SslError {
    fn from(x: IoError) -> Self {
        SslError::Io(x)
    }
}

impl From<HandshakeError<TcpStream>> for SslError {
    fn from(x: HandshakeError<TcpStream>) -> Self {
        SslError::Handshake(x)
    }
}

impl From<IoError> for FwError<SslError> {
    fn from(x: IoError) -> FwError<SslError> {
        FwError::Io(SslError::Io(x))
    }
}

impl From<ErrorStack> for FwError<SslError> {
    fn from(x: ErrorStack) -> Self {
        FwError::Io(SslError::SslStack(x))
    }
}

impl From<HandshakeError<TcpStream>> for FwError<SslError> {
    fn from(x: HandshakeError<TcpStream>) -> FwError<SslError> {
        FwError::Io(SslError::Handshake(x))
    }
}

impl From<&str> for FwError<SslError> {
    fn from(x: &str) -> FwError<SslError> {
        FwError::Io(SslError::Str(x.to_string()))
    }
}


pub struct SslChan {
    #[allow(dead_code)]
    addr: SocketAddr,
    stream: SslStream<TcpStream>,
}

#[derive(Debug)]
pub struct SslMidChan {
    addr: SocketAddr,
    stream: MidHandshakeSslStream<TcpStream>,
}

pub struct SslListener {
    listener: MioTcpListener,
    acceptor: SslAcceptor,
    conf: StreamConf
}

impl Pollable for SslChan {
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), IoError> {
        poll.register(self.stream.get_ref(), Token(tok), Ready::all(), PollOpt::edge())
    }

    fn deregister(&self, poll: &Poll) -> Result<(), IoError> {
        poll.deregister(self.stream.get_ref())
    }
}

impl Pollable for SslMidChan {
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), IoError> {
        poll.register(self.stream.get_ref(), Token(tok), Ready::all(), PollOpt::edge())
    }

    fn deregister(&self, poll: &Poll) -> Result<(), IoError> {
        poll.deregister(self.stream.get_ref())
    }
}

impl Chan for SslChan {
    type Err = SslError;
    fn send(&mut self, buff: &[u8]) -> Result<usize, FwError<Self::Err>> {
        Ok(self.stream.write(buff)?)
    }
    fn recv(&mut self, buff: &mut [u8]) -> Result<Option<usize>, FwError<Self::Err>> {
        let read = self.stream.read(buff);
        let read = match read {
            Ok(y) => Some(y),
            Err(x) => match x.kind() {
                ErrorKind::WouldBlock => {
                    None
                }
                _ => return Err(x.into())
            }
        };
        return Ok(read);
    }
}

impl MidChan for SslMidChan {
    type Err = SslError;
    type C = SslChan;

    fn try_channel(self, poll: &Poll) -> Result<NextState<Self::Err, Self::C, Self>, FwError<Self::Err>> {
        match self.stream.handshake() {
            Ok(x) => Ok(
                NextState::Active(
                    SslChan { addr: self.addr, stream: x }
                )
            ),
            Err(x) => match x {
                HandshakeError::WouldBlock(mid_stream) => {
                    Ok(NextState::Pending(SslMidChan { addr: self.addr, stream: mid_stream }))
                }
                HandshakeError::Failure(mid_stream) => {
                    poll.deregister(mid_stream.get_ref())?;
                    Err(HandshakeError::Failure(mid_stream).into())
                }
                x => {
                    Err(x.into())
                }
            }
        }
    }
}

impl Pollable for SslListener {
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), IoError> {
        poll.register(&self.listener, Token(tok), Ready::all(), PollOpt::level())
    }

    fn deregister(&self, poll: &Poll) -> Result<(), IoError> {
        poll.deregister(&self.listener)
    }
}

impl SslListener {
    pub fn bind(addr: &SocketAddr, acceptor: SslAcceptor, conf: StreamConf) -> Result<Self, IoError> {
        Ok(SslListener {
            listener: MioTcpListener::bind(addr)?,
            acceptor,
            conf,
        })
    }

    pub fn pkey_from_file(file: &mut Read) -> Result<PKey<Private>, SslError> {
        let mut pkey_bytes = Vec::<u8>::with_capacity(2048);
        file.read_to_end(&mut pkey_bytes)?;
        let res = PKey::<Private>::private_key_from_pem(pkey_bytes.as_ref())?;
        Ok(res)
    }

    pub fn cert_from_file(file: &mut Read) -> Result<X509, SslError> {
        let mut pkey_bytes = Vec::<u8>::with_capacity(2048);
        file.read_to_end(&mut pkey_bytes)?;
        let res = X509::from_pem(pkey_bytes.as_ref())?;
        Ok(res)
    }
}

impl Listener for SslListener {
    type Err = SslError;
    type C = SslChan;
    type PC = SslMidChan;

    fn accept(&mut self) -> Result<Option<NextState<Self::Err, Self::C, Self::PC>>, FwError<Self::Err>> {
        if let Some((stream, addr)) = {
            match self.listener.accept() {
                Err(x) => match x.kind() {
                    ErrorKind::WouldBlock => {
                        Ok(None)
                    }
                    _ => Err(x)
                }
                Ok(x) => Ok(Some(x))
            }?
        } {
            stream.set_nodelay(true)?;

            stream.set_keepalive(self.conf.keepalive)?;
            stream.set_linger(self.conf.linger)?;

            let chan = match self.acceptor.accept(stream) {
                Ok(x) => Ok(
                    Some(NextState::Active(
                        SslChan { addr, stream: x }
                    ))
                ),
                Err(x) => match x {
                    HandshakeError::WouldBlock(mid_stream) => {
                        Ok(Some(NextState::Pending(
                            SslMidChan { addr, stream: mid_stream }
                        )))
                    }
                    x => Err(x.into())
                }
            };

            chan
        } else {
            Ok(None)
        }
    }
}

pub enum SslParseError {
    Stack(ErrorStack),
}

impl Parsable<Result<SslListener, FwError<SslError>>> for SslListener {
    fn parser<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
        let app = app
            .arg(
                Arg::with_name("addr")
                    .required(true)
                    .index(1)
            )
            .arg(
                Arg::with_name("ca")
                    .required(true)
                    .index(2)
            )
            .arg(
                Arg::with_name("cert")
                    .required(true)
                    .index(3)
            )
            .arg(
                Arg::with_name("privkey")
                    .required(true)
                    .index(4)
            );
        StreamConf::parser(app)
    }
    fn parse<'a>(matches: &ArgMatches) -> Result<SslListener, FwError<SslError>> {
        let addr = matches.value_of("addr").ok_or("address not found")?;
        let ca = matches.value_of("ca").ok_or("ca not found")?;
        let cert = matches.value_of("cert").ok_or("cert not found")?;
        let privkey = matches.value_of("privkey").ok_or("privkey not found")?;

        let addr = addr.parse::<SocketAddr>().map_err(|_| "invalid socket address")?;

        let conf = StreamConf::parse(&matches)?;

        let mut acceptor = SslAcceptor::mozilla_intermediate(SslMethod::tls())?;
        acceptor.set_certificate_file(cert, SslFiletype::PEM)?;
        acceptor.set_private_key_file(privkey, SslFiletype::PEM)?;
        acceptor.set_ca_file(ca)?;
        acceptor.check_private_key()?;

        let acceptor = acceptor.build();

        Ok(
            SslListener::bind(
                &addr,
                acceptor,
                conf,
            )?
        )
    }
}