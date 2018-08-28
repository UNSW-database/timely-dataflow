//! Typed inter-thread, intra-process channels.

use std::sync::{Arc, Mutex};
use std::any::Any;
use std::sync::mpsc::{Sender, Receiver, channel};

use allocator::{Allocate, Message, Thread};
use {Push, Pull};

/// An allocater for inter-thread, intra-process communication
pub struct Process {
    inner:      Thread,                         // inner Thread
    index:      usize,                          // number out of peers
    peers:      usize,                          // number of peer allocators (for typed channel allocation).
    allocated:  usize,                          // indicates how many have been allocated (locally).
    channels:   Arc<Mutex<Vec<Box<Any+Send>>>>, // Box<Any+Send> -> Box<Vec<Option<(Vec<Sender<T>>, Receiver<T>)>>>
}

impl Process {
    /// Access the wrapped inner allocator.
    pub fn inner<'a>(&'a mut self) -> &'a mut Thread { &mut self.inner }
    /// Allocate a list of connected intra-process allocators.
    pub fn new_vector(count: usize) -> Vec<Process> {
        let channels = Arc::new(Mutex::new(Vec::new()));
        (0 .. count).map(|index| Process {
            inner:      Thread,
            index:      index,
            peers:      count,
            allocated:  0,
            channels:   channels.clone(),
        }).collect()
    }
}

impl Allocate for Process {
    fn index(&self) -> usize { self.index }
    fn peers(&self) -> usize { self.peers }
    fn allocate<T: Any+Send+'static>(&mut self) -> (Vec<Box<Push<Message<T>>>>, Box<Pull<Message<T>>>, Option<usize>) {

        // ensure exclusive access to shared list of channels
        let mut channels = self.channels.lock().ok().expect("mutex error?");

        // we may need to alloc a new channel ...
        if self.allocated == channels.len() {
            let mut pushers = Vec::new();
            let mut pullers = Vec::new();
            for _ in 0..self.peers {
                let (s, r): (Sender<Message<T>>, Receiver<Message<T>>) = channel();
                pushers.push(Pusher { target: s });
                pullers.push(Puller { source: r, current: None });
            }

            let mut to_box = Vec::new();
            for recv in pullers.into_iter() {
                to_box.push(Some((pushers.clone(), recv)));
            }

            channels.push(Box::new(to_box));
        }

        let vector =
        channels[self.allocated]
            .downcast_mut::<(Vec<Option<(Vec<Pusher<Message<T>>>, Puller<Message<T>>)>>)>()
            .expect("failed to correctly cast channel");

        let (send, recv) =
        vector[self.index]
            .take()
            .expect("channel already consumed");

        self.allocated += 1;
        let mut temp = Vec::new();
        for s in send.into_iter() { temp.push(Box::new(s) as Box<Push<Message<T>>>); }
        (temp, Box::new(recv) as Box<Pull<super::Message<T>>>, None)
    }
}

/// The push half of an intra-process channel.
struct Pusher<T> {
    target: Sender<T>,
}

impl<T> Clone for Pusher<T> {
    fn clone(&self) -> Self {
        Pusher { target: self.target.clone() }
    }
}

impl<T> Push<T> for Pusher<T> {
    #[inline] fn push(&mut self, element: &mut Option<T>) {
        if let Some(element) = element.take() {
            self.target.send(element).unwrap();
        }
    }
}

/// The pull half of an intra-process channel.
struct Puller<T> {
    current: Option<T>,
    source: Receiver<T>,
}

impl<T> Pull<T> for Puller<T> {
    #[inline]
    fn pull(&mut self) -> &mut Option<T> {
        self.current = self.source.try_recv().ok();
        &mut self.current
    }
}
