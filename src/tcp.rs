use mio::tcp::TcpListener as MioTcpListener;
use std::io::{Error, ErrorKind};
use mio::tcp::TcpStream;
use crate::{Listener, PendingChannel, Channel, FwError, Transition, Pollable};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::marker::PhantomData;

type TcpErr = Error;


pub struct TcpChan<'a> {
    addr: String,
    stream: TcpStream,
    phantom: PhantomData<&'a TcpStream>
}

impl <'a>Pollable<'a> for TcpChan<'a> {
    type E = TcpStream;
    fn pollable(&'a self) -> &'a Self::E {
        &self.stream
    }
}


impl <'a>PendingChannel<'a> for TcpChan<'a> {
    type Err = TcpErr;
    type C = TcpChan<'a>;

    fn try_channel(self) -> Result<Transition<'a, Self::Err, Self::C, Self>, FwError<Self::Err>> {
        return Ok(Transition::Ok(TcpChan { addr: self.addr, stream: self.stream, phantom: PhantomData }));
    }
}

impl <'a>Channel<'a> for TcpChan<'a> {
    type Err = TcpErr;
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

pub struct TcpListener<'a> {
    listener: MioTcpListener,
    phantom: PhantomData<&'a MioTcpListener>
}

impl <'a>TcpListener<'a> {
    pub fn bind(addr: &SocketAddr) -> Result<Self, Error> {
        Ok(TcpListener {
            listener: MioTcpListener::bind(addr)?,
            phantom: PhantomData,
        })
    }
}

impl <'a>Pollable<'a> for TcpListener<'a> {
    type E = MioTcpListener;
    fn pollable(&'a self) -> & Self::E {
        &self.listener
    }
}

impl <'a>Listener<'a> for TcpListener<'a> {
    type Err = TcpErr;
    type C = TcpChan<'a>;
    type PC = TcpChan<'a>;

    fn accept<'b>(&'b mut self) -> Result<Option<Self::PC>, FwError<Self::Err>> {
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
                    TcpChan {
                        addr: addr.to_string(),
                        stream: sock,
                        phantom: PhantomData
                    }
                )
            );
        } else {
            return Ok(None);
        }
    }
}