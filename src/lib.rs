use mio::Poll;
use mio::Events;
use mio::Evented;
use std::collections::HashMap;
use std::io::Error as IOError;
use mio::Token;
use mio::Ready;
use mio::PollOpt;
use std::fmt::Debug;
use std::mem;
use std::marker::PhantomData;

#[cfg(test)]
mod tests;

pub mod tcp;
pub mod unix;

#[derive(Debug)]
pub enum FwError<E: Debug> {
    IO(E),
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
}

pub trait Channel<'a> {
    type Err: Debug;
    fn send(&'a mut self, buff: &mut [u8]) -> Result<usize, FwError<Self::Err>>;
    fn recv(&'a mut self, buff: &mut [u8]) -> Result<Option<usize>, FwError<Self::Err>>;
}

pub trait Pollable<'a> {
    type E: Evented;
    /// return a pollable instance to put into `Poll` instance
    fn pollable(&'a self) -> &'a Self::E;
}

pub enum Transition<'a, Err: Debug, C: Channel<'a, Err=Err>, PC: PendingChannel<'a, Err=Err, C=C>> {
    Ok(C),
    Stay(PC),
    _Hidden(PhantomData<&'a usize>),
}

/// This channel is still initializing
pub trait PendingChannel<'a> {
    type Err: Debug;
    type C: Channel<'a, Err=Self::Err>;

    fn try_channel(self) -> Result<Transition<'a, Self::Err, Self::C, Self>, FwError<Self::Err>>
        where Self: std::marker::Sized;
}

pub trait Listener<'a> {
    type Err: Debug;
    type C: Channel<'a, Err=Self::Err>;
    type PC: PendingChannel<'a, Err=Self::Err, C=Self::C>;
    /// accept a single connection and return it
    fn accept<'b>(&'b mut self) -> Result<Option<Self::PC>, FwError<Self::Err>>;
}

pub trait Connector<'a> {
    type Err: Debug;
    type C: Channel<'a, Err=Self::Err>;
    type PC: PendingChannel<'a, C=Self::C, Err=Self::Err>;
    /// create a single connection and return it
    fn connect<'b>(&'b mut self) -> Result<Self::PC, FwError<Self::Err>>;
}

#[derive(Debug)]
pub enum State<
    'a,
    P: Evented, E: Debug,
    A: Channel<'a, Err=E> + Pollable<'a, E=P>,
    B: PendingChannel<'a, Err=E, C=A> + Pollable<'a, E=P>
> {
    Pending(B),
    Active(A),
    Swapping,
    _Hidden(PhantomData<&'a usize>),
}

impl<
    'a,
    P: Evented, E: Debug,
    A: Channel<'a, Err=E> + Pollable<'a, E=P>,
    B: PendingChannel<'a, Err=E, C=A> + Pollable<'a, E=P>
>
State<'a, P, E, A, B> {
    pub fn is_active(&self) -> bool {
        match &self {
            State::Active(_) => true,
            State::Pending(_) => false,
            State::Swapping => unreachable!("must never happen"),
            State::_Hidden(_) => unreachable!("must never happen"),
        }
    }
}


impl<
    'a,
    P: Evented, E: Debug,
    A: Channel<'a, Err=E> + Pollable<'a, E=P>,
    B: PendingChannel<'a, Err=E, C=A> + Pollable<'a, E=P>
>
Pollable<'a> for
State<'a, P, E, A, B> {
    type E = P;

    fn pollable(&'a self) -> &'a Self::E {
        match self {
            State::Active(x) => x.pollable(),
            State::Pending(x) => x.pollable(),
            _ => unreachable!("must never happen"),
        }
    }
}

#[derive(Debug)]
struct Pair<
    'a,
    EL: Evented,
    ES: Evented,
    Le: Debug,
    Lc: Channel<'a, Err=Le> + Pollable<'a, E=EL>,
    Lp: PendingChannel<'a, C=Lc, Err=Le> + Pollable<'a, E=EL>,
    Se: Debug,
    Sc: Channel<'a, Err=Se> + Pollable<'a, E=ES>,
    Sp: PendingChannel<'a, C=Sc, Err=Se> + Pollable<'a, E=ES>
> {
    ca: State<'a, EL, Le, Lc, Lp>,
    cb: State<'a, ES, Se, Sc, Sp>,
    tx: usize,
    rx: usize,
    tok_a: usize,
    tok_b: usize,
    b: Vec<u8>,
    phantom: PhantomData<&'a usize>,
}

impl
<
    'a,
    EL: Evented,
    ES: Evented,
    Le: Debug,
    Lc: Channel<'a, Err=Le> + Pollable<'a, E=EL>,
    Lp: PendingChannel<'a, C=Lc, Err=Le> + Pollable<'a, E=EL>,
    Se: Debug,
    Sc: Channel<'a, Err=Se> + Pollable<'a, E=ES>,
    Sp: PendingChannel<'a, C=Sc, Err=Se> + Pollable<'a, E=ES>
>
Pair<'a, EL, ES, Le, Lc, Lp, Se, Sc, Sp> {
    pub fn is_active(&self) -> bool {
        let mut i = 0;

        if self.ca.is_active() {
            i += 1;
        }

        if self.cb.is_active() {
            i += 1;
        }

        return i <= 1;
    }
}

pub struct Fw<
    'a,
    EL: Evented,
    ES: Evented,
    Le: Debug, Lc: Channel<'a, Err=Le> + Pollable<'a, E=EL>, Lp: PendingChannel<'a, C=Lc, Err=Le> + Pollable<'a, E=EL>,
    Se: Debug, Sc: Channel<'a, Err=Se> + Pollable<'a, E=ES>, Sp: PendingChannel<'a, C=Sc, Err=Se> + Pollable<'a, E=ES>,
    LL: Listener<'a, C=Lc, PC=Lp, Err=Le> + Pollable<'a, E=EL>,
    SS: Connector<'a, C=Sc, PC=Sp, Err=Se>,
>
{
    listener: LL,
    connector: SS,
    conns: HashMap<usize, Pair<'a, EL, ES, Le, Lc, Lp, Se, Sc, Sp>>,
    poll: Poll,
    next_conn_id: usize,

    event_buffer_size: usize,
    client_buffer_size: usize,
}

impl
<
    'a,
    EL: Evented,
    ES: Evented,
    Le: Debug, Lc: Channel<'a, Err=Le> + Pollable<'a, E=EL>, Lp: PendingChannel<'a, C=Lc, Err=Le> + Pollable<'a, E=EL>,
    Se: Debug, Sc: Channel<'a, Err=Se> + Pollable<'a, E=ES>, Sp: PendingChannel<'a, C=Sc, Err=Se> + Pollable<'a, E=ES>,
    LL: Listener<'a, C=Lc, PC=Lp, Err=Le> + Pollable<'a, E=EL>,
    SS: Connector<'a, C=Sc, PC=Sp, Err=Se>,
>
Fw<'a, EL, ES, Le, Lc, Lp, Se, Sc, Sp, LL, SS> {
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
            conns: HashMap::<usize, Pair<'a,EL, ES, Le, Lc, Lp, Se, Sc, Sp>>::with_capacity(capacity),
            next_conn_id: 1,
            event_buffer_size,
            client_buffer_size,
        })
    }

    fn get(&'a mut self, idx: usize) -> Result<&mut Pair<'a, EL, ES, Le, Lc, Lp, Se, Sc, Sp>, FwPairError<Le, Se>> {
        Ok(self.conns.get_mut(&idx).ok_or(FwPairError::Lost)?)
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
        'b,
        P: Evented, E: Debug, A: Channel<'b, Err=E> + Pollable<'b, E=P>,
        B: PendingChannel<'b, Err=E, C=A> + Pollable<'b, E=P>
    >(st: &mut State<'b, P, E, A, B>) -> Result<bool, FwError<E>> {
        let mut prev = State::Swapping;

        mem::swap(&mut prev, st);

        match prev {
            State::Pending(x) => {
                match x.try_channel()? {
                    Transition::Stay(x) => {
                        *st = State::Pending(x);
                        return Ok(false);
                    }
                    Transition::Ok(x) => {
                        *st = State::Active(x);
                        return Ok(true);
                    }
                    Transition::_Hidden(_) => {
                        unreachable!("must never happen")
                    }
                }
            }
            State::Active(_) => {
                unreachable!("must not call try_proceed on Active channel")
            }
            State::Swapping => {
                unreachable!("must never happen")
            }
            State::_Hidden(_) => {
                unreachable!("must never happen")
            }
        }
    }

    fn check_status(&'a mut self, pair: &mut Pair<'a, EL, ES, Le, Lc, Lp, Se, Sc, Sp>) {
        // disable and enable channel polls for now for now.
    }

    fn accept(&'a mut self) -> Result<(), FwPairError<Le, Se>> {
        if let Some(chan_l) = self.listener.accept().map_err(FwPairError::ml)? {
            let chan_s = self.connector.connect().map_err(FwPairError::ms)?;
            let (conn_id, tok_a, tok_b) = self.create_conn_idents();

            let mut pair = Pair {
                ca: State::Pending(chan_l),
                cb: State::Pending(chan_s),
                b: vec![0; self.client_buffer_size],
                tx: 0,
                rx: 0,
                tok_a,
                tok_b,
                phantom: PhantomData,
            };

            self.conns.insert(conn_id.clone(), pair);

            let mut pair = self.get(conn_id)?;
            {
                self.poll.register(pair.ca.pollable(), Token(tok_a), Ready::all(), PollOpt::edge()).unwrap();
                self.poll.register(pair.cb.pollable(), Token(tok_b), Ready::all(), PollOpt::edge()).unwrap();
            }




            let fa = Self::try_proceed(&mut pair.ca).map_err(FwPairError::ml)?;

            let fb = Self::try_proceed(&mut pair.cb).map_err(FwPairError::ms)?;

            if fa || fb {
                self.check_status(&mut pair);
            }
        }
        Ok(())
    }


    fn polled(&'a mut self, idx: usize) -> Result<(), FwPairError<Le, Se>> {
        let (conn_idx, is_a) = Self::tok_to_conn(idx);

        let pair = self.get(conn_idx)?;

        if is_a && !pair.ca.is_active() {
            let fa = Self::try_proceed(&mut pair.ca).map_err(FwPairError::ml)?;

            if fa && !pair.is_active() {
                //self.poll.deregister()
            }
        }

        if !is_a && !pair.cb.is_active() {
            Self::try_proceed(&mut pair.cb).map_err(FwPairError::<Le, Se>::ms);
        }

        Ok(())
    }

    pub fn run(&'a mut self) {
        self.poll.register(self.listener.pollable(), Token(0), Ready::all(), PollOpt::level()).unwrap();

        let mut events = Events::with_capacity(self.event_buffer_size);

        loop {
            self.poll.poll(&mut events, None).unwrap();

            for event in &events {
                match event.token() {
                    Token(0) => {
                        if let Err(x) = self.accept() {
                            dbg!(x);
                        }
                    }
                    Token(idx) => {
                        if let Err(x) = self.polled(idx) {
                            dbg!(x);
                        }
                    }
                }
            }
        }
    }
}