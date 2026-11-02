//!Implementation of [`TaskManager`]
use super::{task::BIG_STRIDE, TaskControlBlock};
use crate::sync::UPSafeCell;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;
///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

/// A Stride scheduler with stable queue ordering for equal scheduling costs.
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task);
    }
    /// Select the minimum stride and charge its next scheduling quantum.
    /// Prepared with Codex assistance; see reports/lab3.md.
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        let (index, minimum) = self
            .ready_queue
            .iter()
            .enumerate()
            .map(|(index, task)| (index, task.inner_exclusive_access().stride))
            .min_by_key(|(_, stride)| *stride)?;
        let task = self.ready_queue.remove(index).unwrap();
        // Subtracting one common origin preserves ordering and prevents overflow.
        for ready in self.ready_queue.iter() {
            ready.inner_exclusive_access().stride -= minimum;
        }
        {
            let mut inner = task.inner_exclusive_access();
            inner.stride = BIG_STRIDE / inner.priority as u64;
        }
        Some(task)
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}
