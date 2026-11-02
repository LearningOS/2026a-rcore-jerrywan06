//! Per-process resource accounting and the Work/Finish deadlock check.
//! Prepared with Codex assistance; see reports/lab5.md.

#[cfg(test)]
extern crate alloc;

use alloc::{vec, vec::Vec};

/// Tracks one resource family independently of other synchronization primitives.
pub struct DeadlockDetector {
    available: Vec<usize>,
    allocation: Vec<Vec<usize>>,
    need: Vec<Vec<usize>>,
}

impl DeadlockDetector {
    /// Create an empty resource ledger.
    pub fn new() -> Self {
        Self {
            available: Vec::new(),
            allocation: Vec::new(),
            need: Vec::new(),
        }
    }

    /// Register a new or recycled thread ID without inheriting old ownership.
    pub fn add_thread(&mut self, tid: usize) {
        while self.allocation.len() <= tid {
            self.allocation.push(vec![0; self.available.len()]);
            self.need.push(vec![0; self.available.len()]);
        }
        self.retire_thread(tid);
    }

    /// Register a newly created resource at its process-local ID.
    pub fn add_resource(&mut self, id: usize, capacity: usize) {
        assert!(id <= self.available.len());
        if id == self.available.len() {
            self.available.push(capacity);
            for row in self.allocation.iter_mut().chain(self.need.iter_mut()) {
                row.push(0);
            }
        } else {
            self.available[id] = capacity;
            for row in self.allocation.iter_mut().chain(self.need.iter_mut()) {
                row[id] = 0;
            }
        }
    }

    /// Record a pending request and roll it back if detection rejects it.
    pub fn request(&mut self, tid: usize, id: usize, enabled: bool) -> bool {
        self.need[tid][id] += 1;
        if enabled && !self.is_safe() {
            self.need[tid][id] -= 1;
            return false;
        }
        true
    }

    /// Record acquisition only once the underlying primitive has granted it.
    pub fn acquired(&mut self, tid: usize, id: usize) {
        assert!(self.need[tid][id] > 0 && self.available[id] > 0);
        self.need[tid][id] -= 1;
        self.available[id] -= 1;
        self.allocation[tid][id] += 1;
    }

    /// Check ownership before unlocking a mutex.
    pub fn owns(&self, tid: usize, id: usize) -> bool {
        self.allocation[tid][id] > 0
    }

    /// Release a held resource, or publish an additional semaphore permit.
    pub fn released(&mut self, tid: usize, id: usize) {
        if self.allocation[tid][id] > 0 {
            self.allocation[tid][id] -= 1;
        }
        self.available[id] += 1;
    }

    /// Forget an exited thread without inventing releases of abandoned resources.
    pub fn retire_thread(&mut self, tid: usize) {
        self.allocation[tid].fill(0);
        self.need[tid].fill(0);
    }

    /// Find whether all recorded pending requests admit a completion sequence.
    pub fn is_safe(&self) -> bool {
        let mut work = self.available.clone();
        let mut finish = vec![false; self.allocation.len()];
        loop {
            let mut progressed = false;
            for tid in 0..finish.len() {
                if !finish[tid]
                    && self.need[tid]
                        .iter()
                        .zip(&work)
                        .all(|(need, free)| need <= free)
                {
                    for (free, held) in work.iter_mut().zip(&self.allocation[tid]) {
                        *free = free.saturating_add(*held);
                    }
                    finish[tid] = true;
                    progressed = true;
                }
            }
            if !progressed {
                return finish.iter().all(|finished| *finished);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DeadlockDetector;

    fn acquire(detector: &mut DeadlockDetector, tid: usize, id: usize) {
        assert!(detector.request(tid, id, true));
        detector.acquired(tid, id);
    }

    #[test]
    fn repeated_lock_is_rejected_without_poisoning_next_request() {
        let mut d = DeadlockDetector::new();
        d.add_thread(0);
        d.add_resource(0, 1);
        acquire(&mut d, 0, 0);
        assert!(!d.request(0, 0, true));
        assert!(d.is_safe());
        d.released(0, 0);
        acquire(&mut d, 0, 0);
    }

    #[test]
    fn cycle_is_rejected_and_waiter_can_continue_after_release() {
        let mut d = DeadlockDetector::new();
        d.add_thread(0);
        d.add_thread(1);
        d.add_resource(0, 1);
        d.add_resource(1, 1);
        acquire(&mut d, 0, 0);
        acquire(&mut d, 1, 1);
        assert!(d.request(0, 1, true));
        assert!(!d.request(1, 0, true));
        d.released(1, 1);
        d.acquired(0, 1);
        assert!(d.is_safe());
    }

    #[test]
    fn multiple_instances_allow_a_completion_sequence() {
        let mut d = DeadlockDetector::new();
        for tid in 0..4 {
            d.add_thread(tid);
        }
        d.add_resource(0, 2);
        d.add_resource(1, 2);
        acquire(&mut d, 0, 1);
        acquire(&mut d, 1, 0);
        acquire(&mut d, 2, 0);
        acquire(&mut d, 3, 1);
        assert!(d.request(0, 0, true));
        assert!(d.request(2, 1, true));
        assert!(d.is_safe());
    }

    #[test]
    fn handoff_remains_pending_until_the_waiter_resumes() {
        let mut d = DeadlockDetector::new();
        d.add_resource(0, 1);
        d.add_thread(0);
        d.add_thread(1);
        acquire(&mut d, 0, 0);
        assert!(d.request(1, 0, true));
        d.released(0, 0);
        assert!(!d.owns(1, 0));
        d.acquired(1, 0);
        assert!(d.owns(1, 0));
    }

    #[test]
    fn accounting_survives_disabling_detection_and_recycled_tids() {
        let mut d = DeadlockDetector::new();
        d.add_thread(0);
        d.add_resource(0, 1);
        acquire(&mut d, 0, 0);
        assert!(d.request(0, 0, false));
        assert!(!d.is_safe());
        d.retire_thread(0);
        d.add_thread(0);
        assert!(!d.request(0, 0, true));
        d.released(0, 0);
        acquire(&mut d, 0, 0);
    }

    #[test]
    fn new_resources_and_threads_preserve_existing_allocations() {
        let mut d = DeadlockDetector::new();
        d.add_thread(0);
        d.add_resource(0, 1);
        acquire(&mut d, 0, 0);
        d.add_resource(1, 3);
        d.add_thread(1);
        assert!(d.owns(0, 0));
        acquire(&mut d, 1, 1);
        assert!(d.is_safe());
    }
}
