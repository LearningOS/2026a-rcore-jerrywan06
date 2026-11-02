//! File and filesystem-related syscalls
use crate::config::PAGE_SIZE;
use crate::fs::{link_file, open_file, unlink_file, OpenFlags, Stat};
use crate::mm::{translated_user_buffer, translated_user_string, PTEFlags, UserBuffer};
use crate::task::{current_task, current_user_token};

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        let Some(buffers) = translated_user_buffer(token, buf, len, PTEFlags::R) else {
            return -1;
        };
        file.write(UserBuffer::new(buffers)) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        if len == 0 {
            return 0;
        }
        let Some(buffers) = translated_user_buffer(token, buf, len, PTEFlags::W) else {
            return -1;
        };
        file.read(UserBuffer::new(buffers)) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let Some(path) = translated_user_string(token, path, PAGE_SIZE) else {
        return -1;
    };
    let Some(flags) = OpenFlags::from_bits(flags) else {
        return -1;
    };
    if let Some(inode) = open_file(path.as_str(), flags) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

/// Copy file metadata to a checked user buffer, including across page boundaries.
/// Prepared with Codex assistance; see reports/lab4.md.
pub fn sys_fstat(fd: usize, st: *mut Stat) -> isize {
    let task = current_task().unwrap();
    let (file, token) = {
        let inner = task.inner_exclusive_access();
        let Some(Some(file)) = inner.fd_table.get(fd) else {
            return -1;
        };
        (file.clone(), inner.memory_set.token())
    };
    let Some(buffers) = translated_user_buffer(
        token,
        st as *const u8,
        core::mem::size_of::<Stat>(),
        PTEFlags::W,
    ) else {
        return -1;
    };
    let stat = file.stat();
    let mut data = [0u8; core::mem::size_of::<Stat>()];
    data[..8].copy_from_slice(&stat.dev.to_ne_bytes());
    data[8..16].copy_from_slice(&stat.ino.to_ne_bytes());
    data[16..20].copy_from_slice(&stat.mode.bits().to_ne_bytes());
    data[20..24].copy_from_slice(&stat.nlink.to_ne_bytes());
    let mut offset = 0;
    for buffer in buffers {
        buffer.copy_from_slice(&data[offset..offset + buffer.len()]);
        offset += buffer.len();
    }
    0
}

/// Create a checked hard link in the root directory.
pub fn sys_linkat(old_name: *const u8, new_name: *const u8) -> isize {
    let token = current_user_token();
    let Some(old_name) = translated_user_string(token, old_name, PAGE_SIZE) else {
        return -1;
    };
    let Some(new_name) = translated_user_string(token, new_name, PAGE_SIZE) else {
        return -1;
    };
    if link_file(old_name.as_str(), new_name.as_str()) {
        0
    } else {
        -1
    }
}

/// Remove a checked name from the root directory.
pub fn sys_unlinkat(name: *const u8) -> isize {
    let Some(name) = translated_user_string(current_user_token(), name, PAGE_SIZE) else {
        return -1;
    };
    if unlink_file(name.as_str()) {
        0
    } else {
        -1
    }
}
