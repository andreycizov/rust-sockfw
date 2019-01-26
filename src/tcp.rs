use mio::tcp::TcpListener as MioTcpListener;
use std::io::{Error, ErrorKind};
use mio::tcp::TcpStream;
use crate::{Listener, MidChan, Chan, FwError, NextState, Pollable};
use std::io::{Read, Write};
use std::net::SocketAddr;
use mio::{Poll, Token, Ready, PollOpt};

type TcpErr = Error;


pub struct TcpChan {
    addr: String,
    stream: TcpStream,
}

impl Pollable for TcpChan {
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), Error> {
        poll.register(&self.stream, Token(tok), Ready::all(), PollOpt::edge())
    }

    fn deregister(&self, poll: &Poll) -> Result<(), Error> {
        poll.deregister(&self.stream)
    }
}


impl MidChan for TcpChan {
    type Err = TcpErr;
    type C = TcpChan;

    fn try_channel(self) -> Result<NextState<Self::Err, Self::C, Self>, FwError<Self::Err>> {
        return Ok(NextState::Active(TcpChan { addr: self.addr, stream: self.stream }));
    }
}

impl Chan for TcpChan {
    type Err = TcpErr;
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

pub struct TcpListener {
    listener: MioTcpListener,
}

impl TcpListener {
    pub fn bind(addr: &SocketAddr) -> Result<Self, Error> {
        Ok(TcpListener {
            listener: MioTcpListener::bind(addr)?,
        })
    }
}

impl Pollable for TcpListener {
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), Error> {
        poll.register(&self.listener, Token(tok), Ready::all(), PollOpt::level())
    }

    fn deregister(&self, poll: &Poll) -> Result<(), Error> {
        poll.deregister(&self.listener)
    }
}

impl Listener for TcpListener {
    type Err = TcpErr;
    type C = TcpChan;
    type PC = TcpChan;

    fn accept(&mut self) -> Result<Option<NextState<Self::Err, Self::C, Self::PC>>, FwError<Self::Err>> {
        if let Some((sock, addr)) = {
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
            sock.set_nodelay(true)?;
            sock.set_linger(None)?;

            return Ok(
                Some(
                    NextState::Pending(TcpChan {
                        addr: addr.to_string(),
                        stream: sock,
                    })
                )
            );
        } else {
            return Ok(None);
        }
    }
}