use std::sync::{Mutex, Condvar};
use std::collections::VecDeque;

#[derive(Debug)]
pub struct PieceQueue {
    queue: Mutex<VecDeque<u32>>,
    cond: Condvar,
    finished: Mutex<bool>,
}

impl PieceQueue {
    pub fn new(piece_count: u32) -> Self {
        let mut queue = VecDeque::new();
        for i in 0..piece_count {
            queue.push_back(i);
        }
        Self {
            queue: Mutex::new(queue),
            cond: Condvar::new(),
            finished: Mutex::new(false),
        }
    }

    pub fn empty() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
            cond: Condvar::new(),
            finished: Mutex::new(false),
        }
    }

    pub fn pop(&self) -> Option<u32> {
        let mut queue = self.queue.lock().unwrap();
        loop {
            if let Some(piece) = queue.pop_front() {
                return Some(piece);
            }
            if *self.finished.lock().unwrap() {
                return None;
            }
            queue = self.cond.wait(queue).unwrap();
        }
    }

    pub fn push(&self, piece_index: u32) {
        let mut queue = self.queue.lock().unwrap();
        queue.push_back(piece_index);
        self.cond.notify_one();
    }

    pub fn shutdown(&self) {
        let mut finished = self.finished.lock().unwrap();
        *finished = true;
        self.cond.notify_all();
    }

    pub fn is_shutdown(&self) -> bool {
        *self.finished.lock().unwrap()
    }
}
