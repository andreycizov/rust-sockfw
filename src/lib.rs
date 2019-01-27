use mio::Poll;
use mio::Events;
use std::collections::HashMap;
use std::io::Error as IOError;
use mio::Token;
use std::fmt::Debug;
use std::mem;

#[cfg(test)]
mod tests;

pub mod proto;
pub mod args;

#[derive(Debug)]
pub enum FwError<E: Debug> {
    IO(E),
    Register(IOError),
    Disconnected,
    Lost,
    Custom(usize),
}

impl<E: Debug> From<E> for FwError<E> {
    fn from(x: E) -> Self {
        FwError::IO(x)
    }
}

#[derive(Debug)]
pub enum FwPairError<Le: Debug, Se: Debug> {
    L(FwError<Le>),
    S(FwError<Se>),
    Disconnected,
    Lost,
}

impl<Le: Debug, Se: Debug> FwPairError<Le, Se> {
    pub fn ml(x: FwError<Le>) -> Self {
        FwPairError::L(x)
    }

    pub fn l(x: Le) -> Self {
        FwPairError::L(FwError::IO(x))
    }

    pub fn ms(x: FwError<Se>) -> Self {
        FwPairError::S(x)
    }

    pub fn s(x: Se) -> Self {
        FwPairError::S(FwError::IO(x))
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

pub trait Chan {
    type Err: Debug;
    fn send(&mut self, buff: &[u8]) -> Result<usize, FwError<Self::Err>>;
    fn recv(&mut self, buff: &mut [u8]) -> Result<Option<usize>, FwError<Self::Err>>;
}

pub trait Pollable {
    /// return a pollable instance to put into `Poll` instance
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), IOError>;
    fn deregister(&self, poll: &Poll) -> Result<(), IOError>;
}

/// This channel is still initializing
pub trait MidChan {
    type Err: Debug;
    type C: Chan<Err=Self::Err>;

    fn try_channel(self) -> Result<NextState<Self::Err, Self::C, Self>, FwError<Self::Err>>
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

#[derive(Debug)]
pub enum NextState<
    E: Debug,
    A: Chan<Err=E>,
    B: MidChan<Err=E, C=A>
> {
    Pending(B),
    Active(A),
}

#[derive(Debug)]
pub enum State<
    E: Debug,
    A: Chan<Err=E> + Pollable,
    B: MidChan<Err=E, C=A> + Pollable
> {
    Pending(B),
    Active(A),
    Swapping,
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
            State::Swapping => unreachable!("must never happen"),
        }
    }

    pub fn chan(&mut self) -> &mut impl Chan<Err=E> {
        match self {
            State::Active(x) => x,
            _=> unreachable!("must never happen"),
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
    fn register(&self, poll: &Poll, tok: usize) -> Result<(), IOError> {
        return match self {
            State::Active(x) => x.register(&poll, tok),
            State::Pending(x) => x.register(&poll, tok),
            _ => unreachable!("must never happen"),
        };
    }

    fn deregister(&self, poll: &Poll) -> Result<(), IOError> {
        return match self {
            State::Active(x) => x.deregister(&poll),
            State::Pending(x) => x.deregister(&poll),
            _ => unreachable!("must never happen"),
        };
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

impl
<
    Le: Debug, Lc: Chan<Err=Le> + Pollable, Lp: MidChan<C=Lc, Err=Le> + Pollable,
    Se: Debug, Sc: Chan<Err=Se> + Pollable, Sp: MidChan<C=Sc, Err=Se> + Pollable,
    LL: Listener<C=Lc, PC=Lp, Err=Le> + Pollable,
    SS: Connector<C=Sc, PC=Sp, Err=Se>,
>
Fw<Le, Lc, Lp, Se, Sc, Sp, LL, SS> {
    pub fn new(
        listener: LL,
        connector: SS,
        capacity: usize,
        event_buffer_size: usize,
        client_buffer_size: usize,
    ) -> Result<Self, IOError> {
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
    >(st: &mut State<E, A, B>) -> Result<bool, FwError<E>> {
        let mut prev = State::Swapping;

        mem::swap(&mut prev, st);

        match prev {
            State::Pending(x) => {
                match x.try_channel()? {
                    NextState::Pending(x) => {
                        *st = State::Pending(x);
                        Ok(false)
                    }
                    NextState::Active(x) => {
                        *st = State::Active(x);
                        Ok(true)
                    }
                }
            }
            State::Active(_) => {
                unreachable!("must not call try_proceed on Active channel")
            }
            State::Swapping => {
                unreachable!("must never happen")
            }
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

        if actives < 2{
            // we may receive two events from the same Events but to the same channel
            // one could think we'd need to keep the status of the channels active
            // but we already change activity flag in try_proceed
            if is_a && !pair.ca.is_active() {
                let f = Self::try_proceed(&mut pair.ca).map_err(FwPairError::ml)?;

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
                let f = Self::try_proceed(&mut pair.cb).map_err(FwPairError::ms)?;

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
                                other_err => {
                                    dbg!(("other_err", conn_idx, other_err));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
