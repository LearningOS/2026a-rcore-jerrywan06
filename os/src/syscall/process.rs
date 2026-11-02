//! Process management syscalls
use crate::mm::{translated_user_buffer, MapPermission, PTEFlags};
use crate::task::{
    change_program_brk, current_user_token, exit_current_and_run_next, mmap_current,
    munmap_current, suspend_current_and_run_next, syscall_count,
};
use crate::timer::get_time_us;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// Write the current time to a checked user buffer, including across page boundaries.
/// Prepared with Codex assistance; see reports/lab2.md.
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let Some(buffers) = translated_user_buffer(
        current_user_token(),
        ts as *const u8,
        core::mem::size_of::<TimeVal>(),
        PTEFlags::W,
    ) else {
        return -1;
    };
    let us = get_time_us();
    let fields = [
        (us / 1_000_000).to_ne_bytes(),
        (us % 1_000_000).to_ne_bytes(),
    ];
    let mut bytes = fields.iter().flatten();
    for buffer in buffers {
        for byte in buffer {
            *byte = *bytes.next().unwrap();
        }
    }
    0
}

/// Read or write a checked user byte, or query the current task's syscall count.
/// Follows https://learningos.github.io/rCore-Tutorial-Guide/chapter4/7exercise.html.
/// Prepared with Codex assistance; see reports/lab2.md.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        0 | 1 => {
            let permission = if trace_request == 0 {
                PTEFlags::R
            } else {
                PTEFlags::W
            };
            let Some(mut buffers) =
                translated_user_buffer(current_user_token(), id as *const u8, 1, permission)
            else {
                return -1;
            };
            if trace_request == 0 {
                buffers[0][0] as isize
            } else {
                buffers[0][0] = data as u8;
                0
            }
        }
        2 => syscall_count(id) as isize,
        _ => -1,
    }
}

/// Map anonymous user memory with the requested read/write/execute permissions.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap");
    if prot & !0x7 != 0 || prot & 0x7 == 0 {
        return -1;
    }
    let permission = MapPermission::from_bits((prot << 1) as u8).unwrap();
    if mmap_current(start, len, permission) {
        0
    } else {
        -1
    }
}

/// Unmap a page-aligned user range and release its physical frames.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    if munmap_current(start, len) {
        0
    } else {
        -1
    }
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
