use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};

#[derive(Debug)]
pub struct PieceQueue {
    state: Mutex<QueueState>,
    cond: Condvar,
    total_pieces: u32,
}

#[derive(Debug)]
struct QueueState {
    queue: VecDeque<u32>,
    completed: u32,
    finished: bool,
}

impl PieceQueue {
    pub fn new(piece_ids: &Vec<u32>) -> Self {
        let mut queue = VecDeque::new();
        for i in piece_ids {
            queue.push_back(*i);
        }
        Self {
            state: Mutex::new(QueueState {
                queue,
                completed: 0,
                finished: false,
            }),
            cond: Condvar::new(),
            total_pieces: piece_ids.len() as u32,
        }
    }

    pub fn empty() -> Self {
        Self::new(&vec![])
    }

    pub fn pop(&self) -> Option<u32> {
        let mut state = self.state.lock().unwrap();
        loop {
            if let Some(piece) = state.queue.pop_front() {
                return Some(piece);
            }
            if state.finished {
                return None;
            }
            state = self.cond.wait(state).unwrap();
        }
    }

    pub fn push(&self, piece_index: u32) {
        let mut state = self.state.lock().unwrap();
        if state.finished {
            return;
        }
        state.queue.push_back(piece_index);
        self.cond.notify_one();
    }

    pub fn mark_completed(&self) {
        let mut state = self.state.lock().unwrap();
        if state.finished {
            return;
        }
        state.completed += 1;
        if state.completed == self.total_pieces {
            state.finished = true;
            self.cond.notify_all();
        }
    }

    pub fn shutdown(&self) {
        let mut state = self.state.lock().unwrap();
        if state.finished {
            return;
        }
        state.finished = true;
        self.cond.notify_all();
    }

    pub fn is_shutdown(&self) -> bool {
        self.state.lock().unwrap().finished
    }

    pub fn wait_until_finished(&self) {
        let mut state = self.state.lock().unwrap();
        while !state.finished {
            state = self.cond.wait(state).unwrap();
        }
    }
}
