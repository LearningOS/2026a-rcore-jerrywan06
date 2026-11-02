//! Semaphore

use super::{
    acquire_resource, release_resource, request_resource, ResourceKind, UPSafeCell, DEADLOCK_ERROR,
};
use crate::task::{block_current_and_run_next, current_task, wakeup_task, TaskControlBlock};
use alloc::{collections::VecDeque, sync::Arc};

/// semaphore structure
pub struct Semaphore {
    /// semaphore inner
    pub inner: UPSafeCell<SemaphoreInner>,
    resource_id: usize,
}

pub struct SemaphoreInner {
    pub count: isize,
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl Semaphore {
    /// Create a new semaphore
    pub fn new(res_count: usize, resource_id: usize) -> Self {
        trace!("kernel: Semaphore::new");
        Self {
            resource_id,
            inner: unsafe {
                UPSafeCell::new(SemaphoreInner {
                    count: res_count as isize,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }

    /// up operation of semaphore
    pub fn up(&self) -> isize {
        trace!("kernel: Semaphore::up");
        let mut inner = self.inner.exclusive_access();
        if inner.count == isize::MAX {
            return -1;
        }
        inner.count += 1;
        if inner.count <= 0 {
            if let Some(task) = inner.wait_queue.pop_front() {
                wakeup_task(task);
            }
        }
        drop(inner);
        release_resource(ResourceKind::Semaphore, self.resource_id);
        0
    }

    /// down operation of semaphore
    pub fn down(&self) -> isize {
        trace!("kernel: Semaphore::down");
        if !request_resource(ResourceKind::Semaphore, self.resource_id) {
            return DEADLOCK_ERROR;
        }
        let mut inner = self.inner.exclusive_access();
        inner.count -= 1;
        if inner.count < 0 {
            inner.wait_queue.push_back(current_task().unwrap());
            drop(inner);
            block_current_and_run_next();
        } else {
            drop(inner);
        }
        acquire_resource(ResourceKind::Semaphore, self.resource_id);
        0
    }
}
