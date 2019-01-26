use mio::Poll;
use mio::Events;
use mio::Evented;
use std::collections::HashMap;
use std::io::Error as IOError;
use mio::Token;
use mio::Ready;
use mio::PollOpt;
use std::fmt::Debug;

#[cfg(test)]
mod tests;

pub mod tcp;
pub mod unix;

#[derive(Debug)]
pub enum FwError<E: Debug> {
    IO(E),
    Disconnected,
    Lost,
}

impl<E: Debug> From<E> for FwError<E> {
    fn from(x: E) -> Self {
        FwError::IO(x)
    }
}

pub trait Channel {
    type Err: Debug;
    fn send(&mut self, buff: &mut [u8]) -> Result<usize, FwError<Self::Err>>;
    fn recv(&mut self, buff: &mut [u8]) -> Result<Option<usize>, FwError<Self::Err>>;
}

pub trait Pollable {
    type E: Evented;
    /// return a pollable instance to put into `Poll` instance
    fn pollable(&self) -> &Self::E;
}

pub enum Transition<Err: Debug, C: Channel<Err=Err>, PC: PendingChannel<Err=Err, C=C>> {
    Ok(C),
    Stay(PC),
}

/// This channel is still initializing
pub trait PendingChannel {
    type Err: Debug;
    type C: Channel<Err=Self::Err>;

    fn try_channel(self) -> Result<Transition<Self::Err, Self::C, Self>, FwError<Self::Err>>
        where Self: std::marker::Sized;
}

pub trait Listener {
    type Err: Debug;
    type C: Channel<Err=Self::Err>;
    type PC: PendingChannel<Err=Self::Err, C=Self::C>;
    /// accept a single connection and return it
    fn accept(&mut self) -> Result<Option<Self::PC>, FwError<Self::Err>>;
}

pub trait Connector {
    type Err: Debug;
    type C: Channel<Err=Self::Err>;
    type PC: PendingChannel<C=Self::C, Err=Self::Err>;
    /// create a single connection and return it
    fn connect(&mut self) -> Result<Self::PC, FwError<Self::Err>>;
}

#[derive(Debug)]
pub enum State<
    P: Evented, E: Debug,
    A: Channel<Err=E> + Pollable<E=P>,
    B: PendingChannel<Err=E, C=A> + Pollable<E=P>
> {
    Pending(B),
    Active(A),
}

#[derive(Debug)]
struct Pair<
    EL: Evented,
    ES: Evented,
    Le: Debug,
    Lc: Channel<Err=Le> + Pollable<E=EL>,
    Lp: PendingChannel<C=Lc, Err=Le> + Pollable<E=EL>,
    Se: Debug,
    Sc: Channel<Err=Se> + Pollable<E=ES>,
    Sp: PendingChannel<C=Sc, Err=Se> + Pollable<E=ES>
> {
    ca: State<EL, Le, Lc, Lp>,
    cb: State<ES, Se, Sc, Sp>,
    tx: usize,
    rx: usize,
    tok_a: usize,
    tok_b: usize,
    b: Vec<u8>,
}

pub struct Fw<
    EL: Evented,
    ES: Evented,
    Le: Debug, Lc: Channel<Err=Le> + Pollable<E=EL>, Lp: PendingChannel<C=Lc, Err=Le> + Pollable<E=EL>,
    Se: Debug, Sc: Channel<Err=Se> + Pollable<E=ES>, Sp: PendingChannel<C=Sc, Err=Se> + Pollable<E=ES>,
    LL: Listener<C=Lc, PC=Lp, Err=Le> + Pollable<E=EL>,
    SS: Connector<C=Sc, PC=Sp, Err=Se>,
>
{
    poll: Poll,
    events: Events,
    listener: LL,
    connector: SS,
    conns: HashMap<usize, Pair<EL, ES, Le, Lc, Lp, Se, Sc, Sp>>,
    next_conn_id: usize,

    client_buffer_size: usize,
}

impl
<
    EL: Evented,
    ES: Evented,
    Le: Debug, Lc: Channel<Err=Le> + Pollable<E=EL>, Lp: PendingChannel<C=Lc, Err=Le> + Pollable<E=EL>,
    Se: Debug, Sc: Channel<Err=Se> + Pollable<E=ES>, Sp: PendingChannel<C=Sc, Err=Se> + Pollable<E=ES>,
    LL: Listener<C=Lc, PC=Lp, Err=Le> + Pollable<E=EL>,
    SS: Connector<C=Sc, PC=Sp, Err=Se>,
>
Fw<EL, ES, Le, Lc, Lp, Se, Sc, Sp, LL, SS> {
    pub fn new(
        listener: LL,
        connector: SS,
        capacity: usize,
        client_buffer_size: usize,
    ) -> Result<Self, IOError> {
        Ok(Fw {
            poll: Poll::new()?,
            events: Events::with_capacity(capacity),
            listener,
            connector,
            conns: HashMap::<usize, Pair<EL, ES, Le, Lc, Lp, Se, Sc, Sp>>::with_capacity(capacity),
            next_conn_id: 1,
            client_buffer_size,
        })
    }

    fn create_conn_idents(&mut self) -> (usize, usize, usize) {
        let idx = self.next_conn_id;
        self.next_conn_id += 1;
        let tok_a = idx * 2;
        let tok_b = tok_a + 1;
        (idx, tok_a, tok_b)
    }

    fn try_proceed<
        P: Evented, E: Debug, A: Channel<Err=E> + Pollable<E=P>,
        B: PendingChannel<Err=E, C=A> + Pollable<E=P>
    >(st: &mut State<P, E, A, B>) -> Result<bool, FwError<E>> {
        match &st {
            State::Pending(x) => {
                match &x.try_channel()? {
                    Transition::Stay(x) => {
                        *st = State::Pending(*x);
                        return Ok(false)
                    }
                    Transition::Ok(x) => {
                        *st = State::Active(*x);
                        return Ok(true)
                    }
                }
            }
            State::Active(_) => {
                unreachable!("must not call try_proceed on Active channel")
            }
        }
    }

    fn check_status(pair: &mut Pair<EL, ES, Le, Lc, Lp, Se, Sc, Sp>) {
        // disable and enable channel polls for now for now.
    }

    pub fn run(&mut self) {
        self.poll.register(&self.listener.pollable(), Token(0), Ready::all(), PollOpt::level()).unwrap();

        loop {
            self.poll.poll(&mut self.events, None).unwrap();

            for event in &self.events {
                match event.token() {
                    Token(0) => {
                        let accept = self.listener.accept();
                        if let Ok(Some(chan_l)) = accept {
                            if let Ok(chan_s) = self.connector.connect() {
                                let (conn_id, tok_a, tok_b) = self.create_conn_idents();

                                self.poll.register(&chan_l.pollable(), Token(tok_a), Ready::all(), PollOpt::edge()).unwrap();
                                self.poll.register(&chan_s.pollable(), Token(tok_b), Ready::all(), PollOpt::edge()).unwrap();

                                let pair = Pair {
                                    ca: State::Pending(chan_l),
                                    cb: State::Pending(chan_s),
                                    b: vec![0; self.client_buffer_size],
                                    tx: 0,
                                    rx: 0,
                                    tok_a: tok_a,
                                    tok_b: tok_b,
                                };

                                Self::try_proceed(&mut pair.ca).unwrap();
                                Self::try_proceed(&mut pair.cb).unwrap();

                                self.conns.insert(conn_id.clone(), pair);
                            }
                        } else if let Err(err) = accept {
                            dbg!(err);
                        }
                    }
                }
            }
        }
    }
}