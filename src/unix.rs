use std::io::{Error, ErrorKind, Write, Read};
use std::os::unix::net::UnixStream;

use crate::{FwError, MidChan, Chan, Connector, NextState, Pollable};
use mio::unix::EventedFd;
use std::os::unix::io::AsRawFd;
use mio::{Token, Poll, Ready, PollOpt};
type UnixErr = Error;

pub struct UnixChan {
    #[allow(dead_code)]
    addr: Option<String>,
    stream: UnixStream,
}

impl Pollable for UnixChan {
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), Error> {
        poll.register(&EventedFd(&self.stream.as_raw_fd()), Token(tok), Ready::all(), PollOpt::edge())
    }

    fn deregister(&self, poll: &Poll) -> Result<(), Error> {
        poll.deregister(&EventedFd(&self.stream.as_raw_fd()))
    }
}

impl MidChan for UnixChan {
    type Err = UnixErr;
    type C = UnixChan;

    fn try_channel(self) -> Result<NextState<Self::Err, Self::C, Self>, FwError<Self::Err>> {
        return Ok(NextState::Active(self));
    }
}

impl Chan for UnixChan {
    type Err = UnixErr;
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

pub struct UnixConnector {
    addr: String
}

impl UnixConnector {
    pub fn new(addr: &str) -> Self {
        UnixConnector { addr: addr.to_string() }
    }
}

impl Connector for UnixConnector {
    type Err = UnixErr;
    type C = UnixChan;
    type PC = UnixChan;

    fn connect(&mut self) -> Result<NextState<Self::Err, Self::C, Self::PC>, FwError<Self::Err>> {
        let conn = UnixStream::connect(&self.addr)?;
        conn.set_nonblocking(true)?;

        return Ok(NextState::Pending(UnixChan { addr: None, stream: conn }));
    }
}