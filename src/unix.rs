use std::io::{Error, ErrorKind, Write, Read};
use std::os::unix::net::UnixStream;

use crate::{FwError, PendingChannel, Channel, Connector, Transition, Pollable};
use mio::unix::EventedFd;
use std::os::unix::io::AsRawFd;
use std::marker::PhantomData;

type UnixErr = Error;

pub struct UnixChan<'a> {
    addr: Option<String>,
    stream: UnixStream,
    phantom: PhantomData<&'a UnixStream>
}

impl <'a>Pollable<'a> for UnixChan<'a> {
    type E = EventedFd<'a>;

    fn pollable(&'a self) -> &'a Self::E {
        &EventedFd(&self.stream.as_raw_fd())
    }
}

impl <'a>PendingChannel<'a> for UnixChan<'a> {
    type Err = UnixErr;
    type C = UnixChan<'a>;

    fn try_channel(self) -> Result<Transition<'a, Self::Err, Self::C, Self>, FwError<Self::Err>> {
        return Ok(Transition::Ok(self));
    }
}

impl <'a>Channel<'a> for UnixChan<'a> {
    type Err = UnixErr;
    fn send(&mut self, buff: &mut [u8]) -> Result<usize, FwError<Self::Err>> {
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

impl <'a>Connector<'a> for UnixConnector {
    type Err = UnixErr;
    type C = UnixChan<'a>;
    type PC = UnixChan<'a>;

    fn connect<'b>(&'b mut self) -> Result<Self::PC, FwError<Self::Err>> {
        let conn = UnixStream::connect(&self.addr)?;
        conn.set_nonblocking(true)?;

        return Ok(UnixChan { addr: None, stream: conn, phantom: PhantomData } )
    }
}