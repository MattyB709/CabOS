use alloc::sync::Arc;

use super::SyscallContext;
use crate::thread::{Thread, suspend_to_thread, IDLE};
use crate::sync::MutexLike;

pub fn sys_exit(thread: &Arc<Thread>, ctx: &impl SyscallContext) -> u64 {
    let process = thread.process.get().unwrap();
    if process.live_threads.lock().len() == 1 {
        let exit_code = ctx.arg0() as i32;
        process.exit_code.set(exit_code);
    }
    suspend_to_thread(IDLE.get().unwrap().clone());
}

pub fn sys_getpid(thread: &Arc<Thread>, _ctx: &impl SyscallContext) -> u64 {
    thread.process.get().unwrap().get_pid() as u64
}

pub fn sys_clone(_thread: &Arc<Thread>, _ctx: &impl SyscallContext) -> u64 {
    0 // Unimplemented
}
