use std::sync::{Mutex, Condvar};
use std::collections::VecDeque;

#[derive(Debug)]
pub struct PieceQueue {
    queue: Mutex<VecDeque<u32>>,
    cond: Condvar,
    finished: Mutex<bool>,
    total_pieces: u32,
    completed_pieces: Mutex<u32>,
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
            total_pieces: piece_count,
            completed_pieces: Mutex::new(0),
        }
    }

    pub fn empty() -> Self {
        Self::new(0)
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

    pub fn mark_completed(&self) {
        let mut completed = self.completed_pieces.lock().unwrap();
        println!("[Queue] Completed pieces: {}/{}", *completed + 1, self.total_pieces);
        *completed += 1;
        if *completed == self.total_pieces {
            self.shutdown();
        }
    }

    pub fn shutdown(&self) {
        let mut finished = self.finished.lock().unwrap();
        *finished = true;
        self.cond.notify_all();
    }

    pub fn is_shutdown(&self) -> bool {
        *self.finished.lock().unwrap()
    }
    
    pub fn wait_until_finished(&self) {
        let mut finished = self.finished.lock().unwrap();
        while !*finished {
            finished = self.cond.wait(finished).unwrap();
        }
    }
}
