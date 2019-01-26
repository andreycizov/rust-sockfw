use mio::tcp::TcpListener;
use std::net::SocketAddr;
use mio::{Poll, Token, Ready, PollOpt, Events};
use mio::unix::EventedFd;
use std::os::unix::net::UnixStream;
use std::os::unix::io::AsRawFd;
use mio::tcp::TcpStream;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::io::{Error as IOError, ErrorKind};
use std::fmt::{Debug, Error as FmtError, Formatter};
use mio::Evented;


#[derive(Debug)]
pub enum FWError {
    IO(IOError),
    Lost,
}

impl From<IOError> for FWError {
    fn from(x: IOError) -> Self {
        FWError::IO(x)
    }
}


fn get(maps: &mut MAP, idx: usize) -> Result<&mut CK, FWError> {
    Ok(maps.get_mut(&idx).ok_or(FWError::Lost)?)
}

fn handle_rw<R: Read, W: Write>(buffer: &mut [u8], chan_a: &mut R, chan_b: &mut W) -> Result<Option<usize>, FWError> {
    let read = chan_a.read(buffer);

    let read = match read {
        Ok(y) => Some(y),
        Err(x) => match x.kind() {
            ErrorKind::WouldBlock => {
                None
            }
            _ => return Err(x.into())
        }
    };

    if let Some(x) = read {
        if x > 0 {
            chan_b.write(&buffer[..x])?;
        } else {
            return Err(FWError::Lost);
        }
    };

    Ok(read)
}

fn handle_once(maps: &mut MAP, idx: usize, is_a: bool) -> Result<Option<usize>, FWError> {
    let chan = get(maps, idx)?;

    let read = if is_a {
        handle_rw(&mut chan.b.as_mut(), &mut chan.ca, &mut chan.cb)?
    } else {
        handle_rw(&mut chan.b.as_mut(), &mut chan.cb, &mut chan.ca)?
    };

    if let Some(x) = read {
        let change = if is_a {
            &mut chan.rx
        } else {
            &mut chan.tx
        };

        *change += x;
    }

    Ok(read)
}

fn handle_many(maps: &mut MAP, idx: usize, is_a: bool) -> Result<(), FWError> {
    while let Some(x) = handle_once(maps, idx, is_a)? {
        dbg!(x);
    }

    Ok(())
}

fn handle_err(maps: &mut MAP, idx: usize, is_a: bool) -> bool {
    match handle_many(maps, idx, is_a) {
        Ok(_) => true,
        Err(x) => {
            dbg!(x);
            false
        }
    }
}

fn deregister(poll: &mut Poll, maps: &mut MAP, idx: usize) -> Result<CK, FWError> {
    println!("disconnect");
    let book = get(maps, idx)?;

    poll.deregister(&book.ca)?;
    poll.deregister(&EventedFd(&book.cb.as_raw_fd()))?;

    let rtn = maps.remove(&idx).ok_or(FWError::Lost)?;

    Ok(rtn)
}


// 0. Received FDs from the listener.
// 1. Connecting (A, B) - both need to synchronise first
// 2. Sending

enum State<E, A: Channel<E>, B: PendingChannel<A, E>> {
    Pending(B),
    Active(A),
    Error(E),
}

struct Pair<A, B> {
    addr: String,
    ca: A,
    cb: B,
    tx: usize,
    rx: usize,
    b: Vec<u8>,
}

impl<A, B> Debug for Pair<A, B> {
    fn fmt(&self, f: &mut Formatter) -> Result<(), FmtError> {
        write!(f, "ChanKeeping({:?}, {:?}, {:?}, {:?})", self.addr, self.tx, self.rx, self.b.len())
    }
}

type CK = Pair<TcpStream, UnixStream>;
type MAP = HashMap<usize, CK>;

trait Channel<Err: Sized> {
    fn send(&mut self, buff: &[u8]) -> Result<usize, Err>;
    fn recv(&mut self, buff: &[u8]) -> Result<usize, Err>;
}

trait Pollable<E: Evented> {
    /// return a pollable instance to put into `Poll` instance
    fn pollable(&self) -> &E;
}

/// This channel is still initializing
trait PendingChannel<C: Channel<Err>, Err> {
    fn try_channel(&mut self) -> Result<Option<C>, Err>;
}

trait Listener<C: Channel<Err>, PC: PendingChannel<C, Err>, Err> {
    /// accept a single connection and return it
    fn accept(&mut self) -> Result<Option<PC>, Err>;
}

trait Connector<C: Channel<Err>, PC: PendingChannel<C, Err>, Err> {
    /// create a single connection and return it
    fn connect(&mut self) -> Result<PC, Err>;
}




struct Fw<SA, SB> {
    poll: Poll,
    events: Events,
    maps: HashMap<usize, Pair<SA, SB>>,
    next_map: usize,

    client_buffer_size: usize,
}

//impl Fw {
//    pub fn new() -> Self {
//
//    }
//}

fn main() {
    let buffer_size = 4096;

    let addr_out = "/var/run/docker.sock";
    let addr_in = &"0.0.0.0:8443".parse::<SocketAddr>().unwrap();

    let mut poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(1024);

    let mut maps = MAP::with_capacity(1024);
    let mut next_map: usize = 1;


    let listener = TcpListener::bind(&addr_in).unwrap();
    poll.register(&listener, Token(0), Ready::all(), PollOpt::level()).unwrap();

    loop {
        poll.poll(&mut events, None).unwrap();

        for event in &events {
            match event.token() {
                Token(0) => {
                    let (connected_a, addr) = match listener.accept() {
                        Err(x) => match x.kind() {
                            ErrorKind::WouldBlock => {
                                continue;
                            }
                            _ => Err(x)
                        }
                        x => x
                    }.unwrap();

                    let connected_b = UnixStream::connect(addr_out).unwrap();

                    connected_a.set_nodelay(true).unwrap();
                    connected_a.set_linger(None).unwrap();
                    //connected_a.set_ttl(None).unwrap();
                    connected_b.set_nonblocking(true).unwrap();

                    //connected_b.set_nodelay(true);

                    let idx = next_map;
                    let idx_a = idx * 2;
                    let idx_b = idx_a + 1;

                    poll.register(&connected_a, Token(idx_a), Ready::all(), PollOpt::edge()).unwrap();
                    poll.register(&EventedFd(&connected_b.as_raw_fd()), Token(idx_b), Ready::all(), PollOpt::edge()).unwrap();

                    maps.insert(
                        idx.clone(),
                        Pair {
                            addr: addr.to_string(),
                            ca: connected_a,
                            cb: connected_b,
                            b: vec![0; buffer_size],
                            tx: 0,
                            rx: 0,
                        },
                    );

                    next_map += 1;
                }
                Token(x) => {
                    let is_a = x % 2 == 0;
                    let idx = x / 2;

                    if !handle_err(&mut maps, idx, is_a) {
                        let rtn = deregister(&mut poll, &mut maps, idx).unwrap();
                    }
                }
            }
        }
    }
}
