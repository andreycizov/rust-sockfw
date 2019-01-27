use mio::tcp::{TcpListener as MioTcpListener, TcpStream};
use std::io::{Error as IoError, ErrorKind, Read, Write};
use std::net::SocketAddr;
use mio::{Poll, Token, Ready, PollOpt};
use clap::{App, Arg, ArgMatches};

use crate::{Listener, MidChan, Chan, FwError, NextState, Pollable};
use crate::args::Parsable;
use crate::proto::common::StreamConf;

#[derive(Debug)]
pub enum TcpErr {
    Io(IoError),
    Str(String),
}

impl From<IoError> for TcpErr {
    fn from(x: IoError) -> Self {
        TcpErr::Io(x)
    }
}

impl From<IoError> for FwError<TcpErr> {
    fn from(x: IoError) -> Self {
        FwError::Io(TcpErr::Io(x))
    }
}

impl From<&str> for FwError<TcpErr> {
    fn from(x: &str) -> FwError<TcpErr> {
        FwError::Io(TcpErr::Str(x.to_string()))
    }
}

pub struct TcpChan {
    addr: String,
    stream: TcpStream,
}

impl Pollable for TcpChan {
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), IoError> {
        poll.register(&self.stream, Token(tok), Ready::all(), PollOpt::edge())
    }

    fn deregister(&self, poll: &Poll) -> Result<(), IoError> {
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
    conf: StreamConf,
}

impl TcpListener {
    pub fn bind(
        addr: &SocketAddr,
        conf: &StreamConf,
    ) -> Result<Self, IoError> {
        Ok(TcpListener {
            listener: MioTcpListener::bind(addr)?,
            conf: conf.clone()
        })
    }
}

impl Pollable for TcpListener {
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), IoError> {
        poll.register(&self.listener, Token(tok), Ready::all(), PollOpt::level())
    }

    fn deregister(&self, poll: &Poll) -> Result<(), IoError> {
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

            sock.set_keepalive(self.conf.keepalive)?;
            sock.set_linger(self.conf.linger)?;

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

impl Parsable<Result<TcpListener, FwError<TcpErr>>> for TcpListener {
    fn parser<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
        let app = app
            .arg(
                Arg::with_name("addr")
                    .required(true)
                    .index(1)
            );

        StreamConf::parser(app)
    }


    fn parse(matches: &ArgMatches) -> Result<TcpListener, FwError<TcpErr>> {
        let addr = matches.value_of("addr").ok_or("address not found")?;
        let addr = addr.parse::<SocketAddr>().map_err(|_| "invalid socket address")?;

        let conf = StreamConf::parse(&matches)?;

        Ok(
            TcpListener::bind(
                &addr,
                &conf
            )?
        )
    }
}
