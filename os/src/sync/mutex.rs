//! Mutex (spin-like and blocking(sleep))

use super::{
    acquire_resource, owns_resource, release_resource, request_resource, ResourceKind, UPSafeCell,
    DEADLOCK_ERROR,
};
use crate::task::TaskControlBlock;
use crate::task::{block_current_and_run_next, suspend_current_and_run_next};
use crate::task::{current_task, wakeup_task};
use alloc::{collections::VecDeque, sync::Arc};

/// Mutex trait
pub trait Mutex: Sync + Send {
    /// Lock the mutex
    fn lock(&self) -> isize;
    /// Unlock the mutex
    fn unlock(&self) -> isize;
}

/// Spinlock Mutex struct
pub struct MutexSpin {
    locked: UPSafeCell<bool>,
    resource_id: usize,
}

impl MutexSpin {
    /// Create a new spinlock mutex
    pub fn new(resource_id: usize) -> Self {
        Self {
            locked: unsafe { UPSafeCell::new(false) },
            resource_id,
        }
    }
}

impl Mutex for MutexSpin {
    /// Lock the spinlock mutex
    fn lock(&self) -> isize {
        trace!("kernel: MutexSpin::lock");
        if !request_resource(ResourceKind::Mutex, self.resource_id) {
            return DEADLOCK_ERROR;
        }
        loop {
            let mut locked = self.locked.exclusive_access();
            if *locked {
                drop(locked);
                suspend_current_and_run_next();
                continue;
            } else {
                *locked = true;
                drop(locked);
                acquire_resource(ResourceKind::Mutex, self.resource_id);
                return 0;
            }
        }
    }

    fn unlock(&self) -> isize {
        trace!("kernel: MutexSpin::unlock");
        if !owns_resource(ResourceKind::Mutex, self.resource_id) {
            return -1;
        }
        let mut locked = self.locked.exclusive_access();
        *locked = false;
        drop(locked);
        release_resource(ResourceKind::Mutex, self.resource_id);
        0
    }
}

/// Blocking Mutex struct
pub struct MutexBlocking {
    inner: UPSafeCell<MutexBlockingInner>,
    resource_id: usize,
}

pub struct MutexBlockingInner {
    locked: bool,
    wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl MutexBlocking {
    /// Create a new blocking mutex
    pub fn new(resource_id: usize) -> Self {
        trace!("kernel: MutexBlocking::new");
        Self {
            resource_id,
            inner: unsafe {
                UPSafeCell::new(MutexBlockingInner {
                    locked: false,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }
}

impl Mutex for MutexBlocking {
    /// lock the blocking mutex
    fn lock(&self) -> isize {
        trace!("kernel: MutexBlocking::lock");
        if !request_resource(ResourceKind::Mutex, self.resource_id) {
            return DEADLOCK_ERROR;
        }
        let mut mutex_inner = self.inner.exclusive_access();
        if mutex_inner.locked {
            mutex_inner.wait_queue.push_back(current_task().unwrap());
            drop(mutex_inner);
            block_current_and_run_next();
        } else {
            mutex_inner.locked = true;
            drop(mutex_inner);
        }
        acquire_resource(ResourceKind::Mutex, self.resource_id);
        0
    }

    /// unlock the blocking mutex
    fn unlock(&self) -> isize {
        trace!("kernel: MutexBlocking::unlock");
        if !owns_resource(ResourceKind::Mutex, self.resource_id) {
            return -1;
        }
        let mut mutex_inner = self.inner.exclusive_access();
        assert!(mutex_inner.locked);
        if let Some(waking_task) = mutex_inner.wait_queue.pop_front() {
            wakeup_task(waking_task);
        } else {
            mutex_inner.locked = false;
        }
        drop(mutex_inner);
        release_resource(ResourceKind::Mutex, self.resource_id);
        0
    }
}
