use std::io::{Error as IoError, ErrorKind, Write, Read};
use mio::{Token, Poll, Ready, PollOpt};
use mio_uds::UnixStream;
use clap::{App, Arg, ArgMatches};

use crate::{FwError, MidChan, Chan, Connector, NextState, Pollable};
use crate::args::Parsable;

#[derive(Debug)]
pub enum UnixErr {
    Io(IoError),
    Str(String)
}

impl From<IoError> for UnixErr {
    fn from(x: IoError) -> Self {
        UnixErr::Io(x)
    }
}

impl From<IoError> for FwError<UnixErr> {
    fn from(x: IoError) -> Self {
        FwError::Io(UnixErr::Io(x))
    }
}

impl From<&str> for FwError<UnixErr> {
    fn from(x: &str) -> FwError<UnixErr> {
        FwError::Io(UnixErr::Str(x.to_string()))
    }
}


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
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), IoError> {
        poll.register(&self.stream, Token(tok), Ready::all(), PollOpt::edge())
    }

    fn deregister(&self, poll: &Poll) -> Result<(), IoError> {
        poll.deregister(&self.stream)
    }
}

impl Pollable for MidUnixChan {
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), IoError> {
        poll.register(&self.stream, Token(tok), Ready::all(), PollOpt::edge())
    }

    fn deregister(&self, poll: &Poll) -> Result<(), IoError> {
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

impl Parsable<Result<UnixConnector, FwError<UnixErr>>> for UnixConnector {
    fn parser<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
        app.arg(
            Arg::with_name("addr")
                .required(true)
                .index(1)
        )
    }
    fn parse<'a>(matches: &ArgMatches) -> Result<UnixConnector, FwError<UnixErr>> {
        let addr = matches.value_of("addr").ok_or("address not found")?;
        Ok(UnixConnector::new(addr))
    }
}