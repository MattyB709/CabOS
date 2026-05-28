use alloc::sync::Arc;

use super::SyscallContext;
use crate::{
    sync::MutexLike,
    thread::{IDLE, Thread, suspend_to_thread},
};

// syscall implementation for exit. Only sets exit code if it is the last thread in the process,
// different from the 
pub fn sys_exit(thread: &Arc<Thread>, ctx: &impl SyscallContext) {
    let process = thread.process.get().unwrap();

    // set the exit code and mark the process as dead if this is the last thread.
    let is_last_thread = {
        let mut live_threads = process.live_threads.lock();

        live_threads.retain(|live| match live.upgrade() {
            Some(live) => !Thread::is_same_thread(&live, thread),
            None => false, // prune stale weak refs too
        });

        live_threads.is_empty()
    };
    if is_last_thread {
        process.exit_code.set(ctx.arg0() as i32);
    }

    suspend_to_thread(IDLE.get().unwrap().clone());
    unreachable!();
}

pub fn sys_getpid(thread: &Arc<Thread>, _ctx: &impl SyscallContext) -> u64 {
    thread.process.get().unwrap().get_pid() as u64
}

pub fn sys_clone(_thread: &Arc<Thread>, _ctx: &impl SyscallContext) -> u64 {
    0 // Unimplemented
}
