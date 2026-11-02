use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;
/// sleep syscall
pub fn sys_sleep(ms: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_sleep",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let expire_ms = get_time_ms() + ms;
    let task = current_task().unwrap();
    add_timer(expire_ms, task);
    block_current_and_run_next();
    0
}
/// mutex create syscall
pub fn sys_mutex_create(blocking: bool) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = process_inner
        .mutex_list
        .iter()
        .position(|item| item.is_none())
        .unwrap_or(process_inner.mutex_list.len());
    let mutex: Arc<dyn Mutex> = if blocking {
        Arc::new(MutexBlocking::new(id))
    } else {
        Arc::new(MutexSpin::new(id))
    };
    process_inner.mutex_deadlock.add_resource(id, 1);
    if id < process_inner.mutex_list.len() {
        process_inner.mutex_list[id] = Some(mutex);
        id as isize
    } else {
        process_inner.mutex_list.push(Some(mutex));
        id as isize
    }
}
/// mutex lock syscall
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_lock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let Some(mutex) = process_inner
        .mutex_list
        .get(mutex_id)
        .and_then(|item| item.as_ref())
        .cloned()
    else {
        return -1;
    };
    drop(process_inner);
    drop(process);
    mutex.lock()
}
/// mutex unlock syscall
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_mutex_unlock",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let Some(mutex) = process_inner
        .mutex_list
        .get(mutex_id)
        .and_then(|item| item.as_ref())
        .cloned()
    else {
        return -1;
    };
    drop(process_inner);
    drop(process);
    mutex.unlock()
}
/// semaphore create syscall
pub fn sys_semaphore_create(res_count: usize) -> isize {
    if res_count > isize::MAX as usize {
        return -1;
    }
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = process_inner
        .semaphore_list
        .iter()
        .position(|item| item.is_none())
        .unwrap_or(process_inner.semaphore_list.len());
    process_inner.semaphore_deadlock.add_resource(id, res_count);
    let semaphore = Some(Arc::new(Semaphore::new(res_count, id)));
    if id < process_inner.semaphore_list.len() {
        process_inner.semaphore_list[id] = semaphore;
    } else {
        process_inner.semaphore_list.push(semaphore);
    }
    id as isize
}
/// semaphore up syscall
pub fn sys_semaphore_up(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_up",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let Some(sem) = process_inner
        .semaphore_list
        .get(sem_id)
        .and_then(|item| item.as_ref())
        .cloned()
    else {
        return -1;
    };
    drop(process_inner);
    drop(process);
    sem.up()
}
/// semaphore down syscall
pub fn sys_semaphore_down(sem_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_semaphore_down",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let Some(sem) = process_inner
        .semaphore_list
        .get(sem_id)
        .and_then(|item| item.as_ref())
        .cloned()
    else {
        return -1;
    };
    drop(process_inner);
    drop(process);
    sem.down()
}
/// condvar create syscall
pub fn sys_condvar_create() -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_create",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let mut process_inner = process.inner_exclusive_access();
    let id = if let Some(id) = process_inner
        .condvar_list
        .iter()
        .enumerate()
        .find(|(_, item)| item.is_none())
        .map(|(id, _)| id)
    {
        process_inner.condvar_list[id] = Some(Arc::new(Condvar::new()));
        id
    } else {
        process_inner
            .condvar_list
            .push(Some(Arc::new(Condvar::new())));
        process_inner.condvar_list.len() - 1
    };
    id as isize
}
/// condvar signal syscall
pub fn sys_condvar_signal(condvar_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_signal",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let Some(condvar) = process_inner
        .condvar_list
        .get(condvar_id)
        .and_then(|item| item.as_ref())
        .cloned()
    else {
        return -1;
    };
    drop(process_inner);
    condvar.signal();
    0
}
/// condvar wait syscall
pub fn sys_condvar_wait(condvar_id: usize, mutex_id: usize) -> isize {
    trace!(
        "kernel:pid[{}] tid[{}] sys_condvar_wait",
        current_task().unwrap().process.upgrade().unwrap().getpid(),
        current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid
    );
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let Some(condvar) = process_inner
        .condvar_list
        .get(condvar_id)
        .and_then(|item| item.as_ref())
        .cloned()
    else {
        return -1;
    };
    let Some(mutex) = process_inner
        .mutex_list
        .get(mutex_id)
        .and_then(|item| item.as_ref())
        .cloned()
    else {
        return -1;
    };
    drop(process_inner);
    drop(process);
    condvar.wait(mutex)
}
/// enable deadlock detection syscall
///
/// Accept only zero or one, preserving accounting when detection is disabled.
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    if enabled > 1 {
        return -1;
    }
    current_process().inner_exclusive_access().deadlock_enabled = enabled == 1;
    0
}
