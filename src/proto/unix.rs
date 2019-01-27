use std::io::{Error, ErrorKind, Write, Read};
use mio::{Token, Poll, Ready, PollOpt};
use mio_uds::UnixStream;


use crate::{FwError, MidChan, Chan, Connector, NextState, Pollable};

type UnixErr = Error;

pub struct UnixChan {
    #[allow(dead_code)]
    addr: Option<String>,
    stream: UnixStream,
}

pub struct MidUnixChan {
    #[allow(dead_code)]
    addr: Option<String>,
    stream: UnixStream,
}

impl Pollable for UnixChan {
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), Error> {
        poll.register(&self.stream, Token(tok), Ready::all(), PollOpt::edge())
    }

    fn deregister(&self, poll: &Poll) -> Result<(), Error> {
        poll.deregister(&self.stream)
    }
}

impl Pollable for MidUnixChan {
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), Error> {
        poll.register(&self.stream, Token(tok), Ready::all(), PollOpt::edge())
    }

    fn deregister(&self, poll: &Poll) -> Result<(), Error> {
        poll.deregister(&self.stream)
    }
}

impl MidChan for MidUnixChan {
    type Err = UnixErr;
    type C = UnixChan;

    fn try_channel(self) -> Result<NextState<Self::Err, Self::C, Self>, FwError<Self::Err>> {
        return Ok(NextState::Active(UnixChan { addr: self.addr, stream: self.stream }));
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
    type PC = MidUnixChan;

    fn connect(&mut self) -> Result<NextState<Self::Err, Self::C, Self::PC>, FwError<Self::Err>> {
        let conn = UnixStream::connect(&self.addr)?;

        return Ok(NextState::Pending(MidUnixChan { addr: None, stream: conn }));
    }
}