use alloc::{string::String, sync::Arc, vec::Vec};

use super::{AT_FDCWD, SyscallContext};
use crate::{
    fs::{
        file::File,
        vfs::{FsError, INodeType, VFS},
    },
    print::kprint,
    sync::MutexLike,
    thread::Thread,
};

// TODO implement proper error handling and return types for syscalls for errno
// const MAX_FD: usize = 1024;
pub const O_CREAT: u64 = 64;
const MAX_STR_LEN: u64 = 4096;

pub fn read_user_string(ptr: u64, ctx: &impl SyscallContext) -> Result<String, &'static str> {
    if !ctx.is_user_address(ptr) {
        return Err("Invalid address");
    }
    let mut s = String::new();
    let mut i = 0;
    loop {
        let addr = ptr + i;
        if !ctx.is_user_address(addr) {
            return Err("Invalid address during string read");
        }
        let c = unsafe { *(addr as *const u8) };
        if c == 0 {
            break;
        }
        s.push(c as char);
        i += 1;
        if i > MAX_STR_LEN {
            return Err("String too long");
        }
    }
    Ok(s)
}

// ABI Decoder Layer

pub fn sys_read(thread: &Arc<Thread>, ctx: &impl SyscallContext) -> u64 {
    do_sys_read(ctx.arg0(), ctx.arg1(), ctx.arg2(), thread, ctx)
}

pub fn sys_write(thread: &Arc<Thread>, ctx: &impl SyscallContext) -> u64 {
    do_sys_write(ctx.arg0(), ctx.arg1(), ctx.arg2(), thread, ctx)
}

pub fn sys_openat(thread: &Arc<Thread>, ctx: &impl SyscallContext) -> u64 {
    do_sys_openat(
        ctx.arg0() as i32,
        ctx.arg1(),
        ctx.arg2(),
        ctx.arg3(),
        thread,
        ctx,
    )
}

pub fn sys_close(thread: &Arc<Thread>, ctx: &impl SyscallContext) -> u64 {
    do_sys_close(ctx.arg0() as i32, thread)
}

pub fn sys_mkdirat(_thread: &Arc<Thread>, _ctx: &impl SyscallContext) -> u64 {
    -1i64 as u64 // Unimplemented
}

pub fn sys_unlinkat(_thread: &Arc<Thread>, _ctx: &impl SyscallContext) -> u64 {
    -1i64 as u64 // Unimplemented
}

pub fn sys_newfstatat(_thread: &Arc<Thread>, _ctx: &impl SyscallContext) -> u64 {
    -1i64 as u64 // Unimplemented
}

pub fn sys_faccessat(_thread: &Arc<Thread>, _ctx: &impl SyscallContext) -> u64 {
    -1i64 as u64 // Unimplemented
}

// Core Implementation Layer

fn insert_fd(fd_table: &mut Vec<Option<Arc<File>>>, file: Arc<File>) -> u64 {

    // technically this wastes 3 slots per process, but it simplifies the logic
    let mut fd = 3;

    if fd_table.len() <= fd {
        fd_table.resize(fd + 1, None);
    }

    while fd_table[fd].is_some() {
        fd += 1;
        if fd == fd_table.len() {
            fd_table.push(None);
        }
    }

    fd_table[fd] = Some(file);
    fd as u64
}

pub fn do_sys_read(
    fd: u64,
    buf_ptr: u64,
    count: u64,
    thread: &Arc<Thread>,
    ctx: &impl SyscallContext,
) -> u64 {
    let fd_table = thread.process.get().unwrap().fd_table.lock();
    if let Some(file) = fd_table.get(fd as usize).and_then(Option::as_ref) {
        if !ctx.is_user_address(buf_ptr) || (count > 0 && !ctx.is_user_address(buf_ptr + count - 1))
        {
            return -1i64 as u64;
        }
        let buf = unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u8, count as usize) };
        match file.read(buf) {
            Ok(n) => n as u64,
            Err(_) => -1i64 as u64,
        }
    } else {
        -1i64 as u64
    }
}

pub fn do_sys_write(
    fd: u64,
    buf_ptr: u64,
    count: u64,
    thread: &Arc<Thread>,
    ctx: &impl SyscallContext,
) -> u64 {
    if fd == 1 || fd == 2 {
        if !ctx.is_user_address(buf_ptr) || (count > 0 && !ctx.is_user_address(buf_ptr + count - 1))
        {
            return -1i64 as u64;
        }
        let buf = unsafe { core::slice::from_raw_parts(buf_ptr as *const u8, count as usize) };
        if let Ok(s) = core::str::from_utf8(buf) {
            kprint!("{}", s);
            return count;
        }
    }

    let fd_table = thread.process.get().unwrap().fd_table.lock();
    if let Some(file) = fd_table.get(fd as usize).and_then(Option::as_ref) {
        if !ctx.is_user_address(buf_ptr) || (count > 0 && !ctx.is_user_address(buf_ptr + count - 1))
        {
            return -1i64 as u64;
        }
        let buf = unsafe { core::slice::from_raw_parts(buf_ptr as *const u8, count as usize) };
        match file.write(buf) {
            Ok(n) => n as u64,
            Err(_) => -1i64 as u64,
        }
    } else {
        -1i64 as u64
    }
}

pub fn do_sys_openat(
    dirfd: i32,
    pathname_ptr: u64,
    flags: u64,
    _mode: u64,
    thread: &Arc<Thread>,
    ctx: &impl SyscallContext,
) -> u64 {
    let pathname = match read_user_string(pathname_ptr, ctx) {
        Ok(s) => s,
        Err(_) => {
            return -1i64 as u64;
        }
    };

    let start_node = if pathname.starts_with('/') {
        VFS.get_root().expect("VFS root not set")
    } else if dirfd == AT_FDCWD {
        thread
            .process
            .get()
            .unwrap()
            .cwd
            .lock()
            .clone()
            .unwrap_or_else(|| VFS.get_root().expect("CWD and VFS root not set"))
    } else {
        let fd_table = thread.process.get().unwrap().fd_table.lock();
        match fd_table.get(dirfd as usize).and_then(Option::as_ref) {
            Some(file) => Arc::clone(&file.vnode),
            None => {
                return -1i64 as u64;
            }
        }
    };

    let components: Vec<&str> = pathname.split('/').filter(|s| !s.is_empty()).collect();
    let mut current = start_node;

    if components.is_empty() {
        let file = Arc::new(File::new(current));
        let mut fd_table = thread.process.get().unwrap().fd_table.lock();
        return insert_fd(&mut fd_table, file);
    }

    for &comp in &components[..components.len() - 1] {
        match current.lookup(comp) {
            Ok(next) => current = next,
            Err(_) => {
                return -1i64 as u64;
            }
        }
    }

    let last_comp = components.last().unwrap();
    let vnode = match current.lookup(last_comp) {
        Ok(vnode) => vnode,
        Err(FsError::NotFound) if (flags & O_CREAT) != 0 => {
            match current.create_child(last_comp, INodeType::File) {
                Ok(vnode) => vnode,
                Err(_) => {
                    return -1i64 as u64;
                }
            }
        }
        Err(_) => {
            return -1i64 as u64;
        }
    };

    let file = Arc::new(File::new(vnode));
    let mut fd_table = thread.process.get().unwrap().fd_table.lock();
    insert_fd(&mut fd_table, file)
}

pub fn do_sys_close(fd: i32, thread: &Arc<Thread>) -> u64 {
    let mut fd_table = thread.process.get().unwrap().fd_table.lock();
    if let Some(file) = fd_table.get_mut(fd as usize) {
        if file.take().is_some() {
            return 0;
        }
    }

    -1i64 as u64
}
