use alloc::sync::Arc;
use core::{mem::size_of, sync::atomic::Ordering};

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use super::SyscallContext;
use crate::{
    arch::{TICKS, TIMER_HZ},
    memory::virtual_memory::{copy_from_user, copy_to_user},
    mp::CoreId,
    thread::{Thread, sleep},
};

const EFAULT: i32 = 14;
const EINVAL: i32 = 22;

const CLOCK_MONOTONIC: i32 = 1;

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

#[derive(IntoBytes, FromBytes, Immutable, Debug, KnownLayout)]
#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}
// linux-style nanosleep. rem is ignored because the kernel has no signal interruption path yet.
// requests are rounded up to MS precision.
pub fn sys_nanosleep(thread: &Arc<Thread>, ctx: &impl SyscallContext) -> Result<(), u64> {
    let req = ctx.arg0();

    // really safe way to prevent integer overflow of the pointer arithmetic below
    let Some(req_end) = req.checked_add((size_of::<Timespec>() - 1) as u64) else {
        return Err(errno(EFAULT));
    };
    if !ctx.is_user_address(req) || !ctx.is_user_address(req_end) {
        return Err(errno(EFAULT));
    }

    // get the timespec struct sent by nanosleep
    let mut bytes = [0_u8; size_of::<Timespec>()];
    if copy_from_user(thread.process.get().unwrap(), req, &mut bytes).is_err() {
        return Err(errno(EFAULT));
    }

    // we're constructing the buffer to be the right size, should be safe to unwrap here
    let timespec = Timespec::read_from_bytes(&bytes).unwrap();
    if timespec.tv_sec < 0 || !(0..1_000_000_000).contains(&timespec.tv_nsec) {
        return Err(errno(EINVAL));
    }

    let duration_ms = (timespec.tv_sec as u64)
        .saturating_mul(1_000)
        .saturating_add((timespec.tv_nsec as u64).div_ceil(1_000_000));
    sleep(thread.clone(), duration_ms);
    Ok(())
}

pub fn sys_clock_gettime(thread: &Arc<Thread>, ctx: &impl SyscallContext) -> u64 {
    let clock_id = ctx.arg0();
    if clock_id != CLOCK_MONOTONIC as u64 {
        errno(EINVAL);
    }
    let timespec_ptr = ctx.arg1();
    let global_ticks = TICKS.read_for(CoreId(0)).load(Ordering::Relaxed);

    let seconds = global_ticks / TIMER_HZ;
    let nanos = (global_ticks % TIMER_HZ) as u128 * 1_000_000_000u128 / TIMER_HZ as u128;
    let timespec = Timespec {
        tv_sec: seconds as i64,
        tv_nsec: nanos as i64,
    };
    if ctx.is_user_address(timespec_ptr)
        && ctx.is_user_address(timespec_ptr + (size_of::<Timespec>() - 1) as u64)
        && copy_to_user(
            thread.process.get().unwrap(),
            timespec_ptr,
            &timespec.as_bytes(),
        )
        .is_ok()
    {
        return 0;
    } else {
        return errno(EFAULT);
    }
}
