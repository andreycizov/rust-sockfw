use mio::Poll;
use mio::Events;
use std::collections::HashMap;
use std::io::Error as IoError;
use mio::Token;
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::mem;
use clap::{App, Arg, ArgMatches};
use crate::args::*;

#[derive(Debug)]
pub enum FwConfError {
    Io(IoError),
    Str(String),
}

#[derive(Debug)]
pub enum FwError<E: Debug> {
    Io(E),
    Register(IoError),
    Disconnected,
    Lost,
    Custom(usize),
}

#[derive(Debug)]
pub enum FwPairError<Le: Debug, Se: Debug> {
    L(FwError<Le>),
    S(FwError<Se>),
    Disconnected,
    Lost,
}

#[derive(Debug)]
pub enum NextState<
    E: Debug,
    A: Chan<Err=E>,
    B: MidChan<Err=E, C=A>
> {
    Pending(B),
    Active(A),
}

pub enum State<
    E: Debug,
    A: Chan<Err=E> + Pollable,
    B: MidChan<Err=E, C=A> + Pollable
> {
    Pending(B),
    Active(A),
    Swapping,
    Lost,
}

impl<
    E: Debug,
    A: Chan<Err=E> + Pollable,
    B: MidChan<Err=E, C=A> + Pollable
> Debug for State<E, A, B> {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        let s = match &self {
            State::Pending(_) => "Pending",
            State::Active(_) => "Active",
            State::Swapping => "Swapping",
            State::Lost => "Lost",
        };

        write!(f, "({})", s)
    }
}

#[derive(Debug)]
struct Pair<
    Le: Debug,
    Lc: Chan<Err=Le> + Pollable,
    Lp: MidChan<C=Lc, Err=Le> + Pollable,
    Se: Debug,
    Sc: Chan<Err=Se> + Pollable,
    Sp: MidChan<C=Sc, Err=Se> + Pollable
> {
    conn_id: usize,
    ca: State<Le, Lc, Lp>,
    cb: State<Se, Sc, Sp>,
    tx: usize,
    rx: usize,
    tok_a: usize,
    tok_b: usize,
    b: Vec<u8>,
}


pub struct FwConf {
    capacity: usize,
    event_buffer_size: usize,
    client_buffer_size: usize,
}

pub struct Fw<
    Le: Debug, Lc: Chan<Err=Le> + Pollable, Lp: MidChan<C=Lc, Err=Le> + Pollable,
    Se: Debug, Sc: Chan<Err=Se> + Pollable, Sp: MidChan<C=Sc, Err=Se> + Pollable,
    LL: Listener<C=Lc, PC=Lp, Err=Le> + Pollable,
    SS: Connector<C=Sc, PC=Sp, Err=Se>,
>
{
    listener: LL,
    connector: SS,
    conns: HashMap<usize, Pair<Le, Lc, Lp, Se, Sc, Sp>>,
    poll: Poll,
    next_conn_id: usize,

    event_buffer_size: usize,
    client_buffer_size: usize,
}


pub trait Chan {
    type Err: Debug;
    fn send(&mut self, buff: &[u8]) -> Result<usize, FwError<Self::Err>>;
    fn recv(&mut self, buff: &mut [u8]) -> Result<Option<usize>, FwError<Self::Err>>;
}

pub trait Pollable {
    /// return a pollable instance to put into `Poll` instance
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), IoError>;
    fn deregister(&self, poll: &Poll) -> Result<(), IoError>;
}

/// This channel is still initializing
pub trait MidChan {
    type Err: Debug;
    type C: Chan<Err=Self::Err>;

    fn try_channel(self, poll: &Poll) -> Result<NextState<Self::Err, Self::C, Self>, FwError<Self::Err>>
        where Self: std::marker::Sized;
}

pub trait Listener {
    type Err: Debug;
    type C: Chan<Err=Self::Err>;
    type PC: MidChan<Err=Self::Err, C=Self::C>;
    /// accept a single connection and return it
    fn accept(&mut self) -> Result<Option<NextState<Self::Err, Self::C, Self::PC>>, FwError<Self::Err>>;
}

pub trait Connector {
    type Err: Debug;
    type C: Chan<Err=Self::Err>;
    type PC: MidChan<C=Self::C, Err=Self::Err>;
    /// create a single connection and return it
    fn connect(&mut self) -> Result<NextState<Self::Err, Self::C, Self::PC>, FwError<Self::Err>>;
}


impl<
    E: Debug,
    A: Chan<Err=E> + Pollable,
    B: MidChan<Err=E, C=A> + Pollable
>
State<E, A, B> {
    pub fn is_active(&self) -> bool {
        match &self {
            State::Active(_) => true,
            State::Pending(_) => false,
            State::Lost => false,
            State::Swapping => unreachable!("must never happen"),
        }
    }

    pub fn chan(&mut self) -> &mut impl Chan<Err=E> {
        match self {
            State::Active(x) => x,
            _=> unreachable!("must never happen 1"),
        }
    }
}

impl<E: Debug, A: Chan<Err=E> + Pollable, B: MidChan<Err=E, C=A> + Pollable>
From<NextState<E, A, B>> for State<E, A, B> {
    fn from(x: NextState<E, A, B>) -> State<E, A, B> {
        match x {
            NextState::Active(x) => State::Active(x),
            NextState::Pending(x) => State::Pending(x),
        }
    }
}


impl<
    E: Debug,
    A: Chan<Err=E> + Pollable,
    B: MidChan<Err=E, C=A> + Pollable
>
Pollable for
State<E, A, B> {
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), IoError> {
        return match self {
            State::Active(x) => x.register(&poll, tok),
            State::Pending(x) => x.register(&poll, tok),
            x => unreachable!("{:?} 2", x),
        };
    }

    fn deregister(&self, poll: &Poll) -> Result<(), IoError> {
        return match self {
            State::Active(x) => x.deregister(&poll),
            State::Pending(x) => x.deregister(&poll),
            State::Lost => Ok(()),
            x => unreachable!("{:?} 3", x),
        };
    }
}

impl
<
    Le: Debug,
    Lc: Chan<Err=Le> + Pollable,
    Lp: MidChan<C=Lc, Err=Le> + Pollable,
    Se: Debug,
    Sc: Chan<Err=Se> + Pollable,
    Sp: MidChan<C=Sc, Err=Se> + Pollable
>
Pair<Le, Lc, Lp, Se, Sc, Sp> {
    pub fn actives(&self) -> usize {
        let mut i = 0;

        if self.ca.is_active() {
            i += 1;
        }

        if self.cb.is_active() {
            i += 1;
        }

        return i
    }
}


impl Parsable<Result<FwConf, FwConfError>> for FwConf {
    fn parser<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
        app
            .arg(
                Arg::with_name("event_buffer_size")
                    .long("event-buffer")
                    .default_value("2048")
                    .required(false)
            )
            .arg(
                Arg::with_name("client_buffer_size")
                    .long("client-buffer")
                    .default_value("8192")
                    .required(false)
            )
            .arg(
                Arg::with_name("capacity")
                    .long("capacity")
                    .default_value("2048")
                    .required(false)
            )
    }

    fn parse<'a>(matches: &ArgMatches) -> Result<FwConf, FwConfError> {
        let event_buffer_size = matches.value_of("event_buffer_size").ok_or("event_buffer_size")?;
        let client_buffer_size = matches.value_of("client_buffer_size").ok_or("client_buffer_size")?;
        let capacity = matches.value_of("capacity").ok_or("capacity")?;

        let event_buffer_size = event_buffer_size.parse::<usize>().map_err(|_| "event_buffer_size")?;
        let client_buffer_size = client_buffer_size.parse::<usize>().map_err(|_| "client_buffer_size")?;
        let capacity = capacity.parse::<usize>().map_err(|_| "capacity")?;

        Ok(
            FwConf { capacity, event_buffer_size, client_buffer_size }
        )
    }
}

impl
<
    Le: Debug, Lc: Chan<Err=Le> + Pollable, Lp: MidChan<C=Lc, Err=Le> + Pollable,
    Se: Debug, Sc: Chan<Err=Se> + Pollable, Sp: MidChan<C=Sc, Err=Se> + Pollable,
    LL: Listener<C=Lc, PC=Lp, Err=Le> + Pollable,
    SS: Connector<C=Sc, PC=Sp, Err=Se>,
>
Fw<Le, Lc, Lp, Se, Sc, Sp, LL, SS> {
    pub fn from_conf(
        conf: &FwConf,
        listener: LL,
        connector: SS,
    ) -> Result<Self, IoError> {
        Self::new(listener, connector, conf.capacity, conf.event_buffer_size, conf.client_buffer_size)
    }

    pub fn new(
        listener: LL,
        connector: SS,
        capacity: usize,
        event_buffer_size: usize,
        client_buffer_size: usize,
    ) -> Result<Self, IoError> {
        Ok(Fw {
            poll: Poll::new()?,
            listener,
            connector,
            conns: HashMap::<usize, Pair<Le, Lc, Lp, Se, Sc, Sp>>::with_capacity(capacity),
            next_conn_id: 1,
            event_buffer_size,
            client_buffer_size,
        })
    }

    fn get(
        conns: &mut HashMap<usize, Pair<Le, Lc, Lp, Se, Sc, Sp>>,
        idx: usize) ->
        Result<&mut Pair<Le, Lc, Lp, Se, Sc, Sp>, FwPairError<Le, Se>> {

        Ok(conns.get_mut(&idx).ok_or(FwPairError::Lost)?)
    }

    fn tok_to_conn(tok_idx: usize) -> (usize, bool) {
        let is_l = tok_idx % 2 == 0;
        let idx = tok_idx / 2;

        (idx, is_l)
    }

    fn create_conn_idents(&mut self) -> (usize, usize, usize) {
        let idx = self.next_conn_id;
        self.next_conn_id += 1;
        let tok_a = idx * 2;
        let tok_b = tok_a + 1;
        (idx, tok_a, tok_b)
    }

    fn try_proceed<
        E: Debug, A: Chan<Err=E> + Pollable,
        B: MidChan<Err=E, C=A> + Pollable
    >(poll: &Poll, st: &mut State<E, A, B>) -> Result<bool, FwError<E>> {
        let mut prev = State::Swapping;

        mem::swap(&mut prev, st);

        match prev {
            State::Pending(x) => {
                match x.try_channel(&poll) {
                    Ok(x) => match x {
                        NextState::Pending(x) => {
                            *st = State::Pending(x);
                            Ok(false)
                        }
                        NextState::Active(x) => {
                            *st = State::Active(x);
                            Ok(true)
                        }
                    }
                    Err(x) => {
                        *st = State::Lost;
                        Err(x)
                    }
                }
            }
            x => unreachable!("{:?}", x)
        }
    }

    fn accept(&mut self) -> Result<(), FwPairError<Le, Se>> {
        if let Some(chan_l) = self.listener.accept().map_err(FwPairError::ml)? {
            let chan_s = self.connector.connect().map_err(FwPairError::ms)?;
            let (conn_id, tok_a, tok_b) = self.create_conn_idents();

            let ca: State<_, _, _> = chan_l.into();
            let cb: State<_, _, _> = chan_s.into();

            ca.register(&self.poll, tok_a).map_err(|x| FwPairError::L(FwError::Register(x)))?;
            cb.register(&self.poll, tok_b).map_err(|x| FwPairError::S(FwError::Register(x)))?;

            let pair = Pair {
                conn_id: conn_id.clone(),
                ca,
                cb,
                b: vec![0; self.client_buffer_size],
                tx: 0,
                rx: 0,
                tok_a,
                tok_b,
            };

            self.conns.insert(conn_id.clone(), pair);
        }
        Ok(())
    }

    fn handle_once <Re: Debug, We: Debug, R: Chan<Err=Re>, W: Chan<Err=We>>
    (buff: &mut [u8], chan_a: &mut R, chan_b: &mut W) -> Result<Option<usize>, FwPairError<Re, We>> {
        let read = chan_a.recv(buff).map_err(FwPairError::ml)?;

        if let Some(x) = read {
            if x > 0 {
                chan_b.send(&buff[..x]).map_err(FwPairError::ms)?;
            } else {
                return Err(FwPairError::Disconnected);
            }
        };

        //dbg!(read);

        Ok(read)
    }

    fn handle_rw <Re: Debug, We: Debug, R: Chan<Err=Re>, W: Chan<Err=We>>
    (buff: &mut [u8], chan_a: &mut R, chan_b: &mut W) -> Result<Option<usize>, FwPairError<Re, We>> {
        let mut total = 0;
        while let Some(x) = Self::handle_once(buff, chan_a, chan_b)? {

            total += x;
        }

        Ok(Some(total))
    }

    fn polled(&mut self, idx: usize) -> Result<(), FwPairError<Le, Se>> {
        let (conn_idx, is_a) = Self::tok_to_conn(idx);

        let pair = Self::get(&mut self.conns, conn_idx)?;

        let actives = pair.actives();

        if actives < 2 {
            // we may receive two events from the same Events but to the same channel
            // one could think we'd need to keep the status of the channels active
            // but we already change activity flag in try_proceed
            if is_a && !pair.ca.is_active() {
                let f = Self::try_proceed(&self.poll, &mut pair.ca).map_err(FwPairError::ml)?;

                if f {
                    if actives == 1 {
                        pair.cb.register(&self.poll, pair.tok_b).map_err(|x| FwPairError::L(FwError::Register(x)))?;
                        //dbg!("enabling S");
                    } else {
                        pair.ca.deregister(&self.poll).map_err(|x| FwPairError::L(FwError::Register(x)))?;
                        //dbg!("pausing L as S not ready");
                    }
                }
            } else if !is_a && !pair.cb.is_active() {
                let f = Self::try_proceed(&self.poll, &mut pair.cb).map_err(FwPairError::ms)?;

                if f {
                    if actives == 1 {
                        pair.ca.register(&self.poll, pair.tok_a).map_err(|x| FwPairError::S(FwError::Register(x)))?;
                        //dbg!("enabling L");
                    } else {
                        pair.cb.deregister(&self.poll).map_err(|x| FwPairError::S(FwError::Register(x)))?;
                        //dbg!("pausing S as L not ready");
                    }
                }
            } {
                //dbg!((is_a, pair.ca.is_active(), pair.cb.is_active()));
            }
        }

        if pair.actives() == 2 {
            if is_a {
                Self::handle_rw(
                    &mut pair.b,
                    pair.ca.chan(),
                    pair.cb.chan(),
                )?;
            } else {
                Self::handle_rw(
                    &mut pair.b,
                    pair.cb.chan(),
                    pair.ca.chan(),
                ).map_err(|x| x.swap())?;
            }

        }

        Ok(())
    }

    pub fn free(&mut self, conn_idx: usize) {
        match Self::get(&mut self.conns, conn_idx) {
            Ok(pair) => {
                if let Err(err_a) = pair.ca.deregister(&self.poll) {
                    dbg!(("err_a", conn_idx, err_a));
                };

                if let Err(err_b) = pair.cb.deregister(&self.poll) {
                    dbg!(("err_b", conn_idx, err_b));
                };
            },
            Err(fetch_error) => {
                dbg!(("fetch_error", conn_idx, fetch_error));
            }
        };

        if self.conns.remove(&conn_idx).is_none() {
            dbg!(("Disconnect", conn_idx, "Already freed"));
        }

        dbg!(("Disconnect", conn_idx, self.conns.len()));
    }

    pub fn run(&mut self) {
        self.listener.register(&self.poll, 0).unwrap();

        let mut events = Events::with_capacity(self.event_buffer_size);

        loop {
            self.poll.poll(&mut events, None).unwrap();

            for event in &events {
                match event.token() {
                    Token(0) => {
                        if let Err(x) = self.accept() {
                            dbg!(("accept", x));
                        }
                    }
                    Token(idx) => {
                        if let Err(err) = self.polled(idx) {
                            let (conn_idx, _) = Self::tok_to_conn(idx);
                            match err {
                                FwPairError::Disconnected => {
                                    self.free(conn_idx);
                                }
                                other_err => {
                                    dbg!(("other_err", conn_idx, other_err));
                                    self.free(conn_idx);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}



impl From<&str> for FwConfError {
    fn from(x: &str) -> FwConfError {
        FwConfError::Str(x.to_string())
    }
}

impl<E: Debug> From<E> for FwError<E> {
    fn from(x: E) -> Self {
        FwError::Io(x)
    }
}

impl<Le: Debug, Se: Debug> FwPairError<Le, Se> {
    pub fn ml(x: FwError<Le>) -> Self {
        FwPairError::L(x)
    }

    pub fn l(x: Le) -> Self {
        FwPairError::L(FwError::Io(x))
    }

    pub fn ms(x: FwError<Se>) -> Self {
        FwPairError::S(x)
    }

    pub fn s(x: Se) -> Self {
        FwPairError::S(FwError::Io(x))
    }

    pub fn swap(self) -> FwPairError<Se, Le> {
        match self {
            FwPairError::L(x) => FwPairError::S(x),
            FwPairError::S(x) => FwPairError::L(x),
            FwPairError::Disconnected => FwPairError::Disconnected,
            FwPairError::Lost => FwPairError::Lost,
        }
    }
}
