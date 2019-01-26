use std::io::{Error, ErrorKind, Write, Read};
use std::os::unix::net::UnixStream;

use crate::{FwError, PendingChannel, Channel, Connector, Transition};

type UnixErr = Error;

struct UnixChan {
    addr: Option<String>,
    stream: UnixStream,
}

impl PendingChannel for UnixChan {
    type Err = UnixErr;
    type C = UnixChan;

    fn try_channel(self) -> Result<Transition<Self::Err, Self::C, Self>, FwError<Self::Err>> {
        return Ok(Transition::Ok(UnixChan { addr: self.addr, stream: self.stream }));
    }
}

impl Channel for UnixChan {
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

struct UnixConnector {
    addr: String
}

impl Connector for UnixConnector {
    type Err = UnixErr;
    type C = UnixChan;
    type PC = UnixChan;

    fn connect(&mut self) -> Result<Self::PC, FwError<Self::Err>> {
        let conn = UnixStream::connect(&self.addr)?;
        conn.set_nonblocking(true)?;

        return Ok(UnixChan { addr: None, stream: conn } )
    }
}