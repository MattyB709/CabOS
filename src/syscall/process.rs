use alloc::sync::Arc;
use core::mem::size_of;

use super::SyscallContext;
use crate::{
    memory::virtual_memory::copy_from_user,
    thread::{Thread, sleep},
};

const EFAULT: i32 = 14;
const EINVAL: i32 = 22;

fn errno(code: i32) -> u64 {
    (-(code as i64)) as u64
}

pub fn sys_exit(thread: &Arc<Thread>, ctx: &impl SyscallContext) -> u64 {
    let exit_code = ctx.arg0() as i32;
    thread.process.get().unwrap().exit_code.set(exit_code);
    0
}

pub fn sys_getpid(thread: &Arc<Thread>, _ctx: &impl SyscallContext) -> u64 {
    thread.process.get().unwrap().get_pid() as u64
}

pub fn sys_clone(_thread: &Arc<Thread>, _ctx: &impl SyscallContext) -> u64 {
    0 // Unimplemented
}

// linux-style nanosleep. rem is ignored because the kernel has no signal interruption path yet.
// requests are rounded up to MS precision.
pub fn sys_nanosleep(thread: &Arc<Thread>, ctx: &impl SyscallContext) -> Result<(), u64> {
    let req = ctx.arg0();

    // really safe way to prevent integer overflow of the pointer arithmetic below
    let Some(req_end) = req.checked_add((size_of::<[u64; 2]>() - 1) as u64) else {
        return Err(errno(EFAULT));
    };
    if !ctx.is_user_address(req) || !ctx.is_user_address(req_end) {
        return Err(errno(EFAULT));
    }

    // get the timespec struct sent by nanosleep
    let mut bytes = [0_u8; size_of::<[u64; 2]>()];
    if copy_from_user(
        thread.process.get().unwrap().get_address_space(),
        req,
        &mut bytes,
    )
    .is_err()
    {
        return Err(errno(EFAULT));
    }

    let seconds = i64::from_ne_bytes(bytes[..8].try_into().unwrap());
    let nanoseconds = i64::from_ne_bytes(bytes[8..].try_into().unwrap());
    if seconds < 0 || !(0..1_000_000_000).contains(&nanoseconds) {
        return Err(errno(EINVAL));
    }

    let duration_ms = (seconds as u64)
        .saturating_mul(1_000)
        .saturating_add((nanoseconds as u64).div_ceil(1_000_000));
    sleep(thread.clone(), duration_ms);
    Ok(())
}
