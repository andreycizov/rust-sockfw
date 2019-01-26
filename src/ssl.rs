use std::io::{Error as IoError, ErrorKind, Read, Write};
use openssl::ssl::{HandshakeError, MidHandshakeSslStream, SslStream, SslAcceptor};
use openssl::error::Error as OrigSslError;
use mio::tcp::{TcpListener as MioTcpListener, TcpStream};
use mio::{Poll, Token, Ready, PollOpt};

use crate::{Listener, MidChan, Chan, FwError, Pollable, NextState};
use std::net::SocketAddr;

#[derive(Debug)]
pub enum SslError {
    Io(IoError),
    Ssl(OrigSslError),
    Handshake(HandshakeError<TcpStream>),
}

impl From<OrigSslError> for SslError {
    fn from(x: OrigSslError) -> Self {
        SslError::Ssl(x)
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
        FwError::IO(SslError::Io(x))
    }
}

impl From<HandshakeError<TcpStream>> for FwError<SslError> {
    fn from(x: HandshakeError<TcpStream>) -> FwError<SslError> {
        FwError::IO(SslError::Handshake(x))
    }
}

pub struct SslChan {
    addr: SocketAddr,
    stream: SslStream<TcpStream>,
}

pub struct SslMidChan {
    addr: SocketAddr,
    stream: MidHandshakeSslStream<TcpStream>,
}

pub struct SslListener {
    listener: MioTcpListener,
    acceptor: SslAcceptor,
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

    fn try_channel(self) -> Result<NextState<Self::Err, Self::C, Self>, FwError<Self::Err>> {
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
                x => Err(x.into())
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
            stream.set_linger(None)?;

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