//! Synchronization and interior mutability primitives

mod condvar;
mod deadlock;
mod mutex;
mod semaphore;
mod up;

pub use condvar::Condvar;
pub(crate) use deadlock::DeadlockDetector;
pub use mutex::{Mutex, MutexBlocking, MutexSpin};
pub use semaphore::Semaphore;
pub use up::UPSafeCell;

use crate::task::{current_process, current_task};

const DEADLOCK_ERROR: isize = -0xdead;

#[derive(Copy, Clone)]
enum ResourceKind {
    Mutex,
    Semaphore,
}

fn with_detector<T>(
    kind: ResourceKind,
    operation: impl FnOnce(&mut DeadlockDetector, usize, bool) -> T,
) -> T {
    let tid = current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid;
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let enabled = inner.deadlock_enabled;
    let detector = match kind {
        ResourceKind::Mutex => &mut inner.mutex_deadlock,
        ResourceKind::Semaphore => &mut inner.semaphore_deadlock,
    };
    operation(detector, tid, enabled)
}

fn request_resource(kind: ResourceKind, id: usize) -> bool {
    with_detector(kind, |detector, tid, enabled| {
        detector.request(tid, id, enabled)
    })
}

fn acquire_resource(kind: ResourceKind, id: usize) {
    with_detector(kind, |detector, tid, _| detector.acquired(tid, id));
}

fn owns_resource(kind: ResourceKind, id: usize) -> bool {
    with_detector(kind, |detector, tid, _| detector.owns(tid, id))
}

fn release_resource(kind: ResourceKind, id: usize) {
    with_detector(kind, |detector, tid, _| detector.released(tid, id));
}
