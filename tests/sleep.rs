#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel_common::test_runner)]

kernel_common::integration_test!({
    use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    use kernel_common::{
        arch::{TIMER_HZ, get_ticks},
        print::kprintln,
        thread::{IDLE, sleep, spawn_thread, suspend_to_thread, this_thread},
    };

    const SLEEP_MS: u64 = 50;

    static FINISHED: AtomicBool = AtomicBool::new(false);
    static ELAPSED_TICKS: AtomicU64 = AtomicU64::new(0);

    kprintln!("Testing sleep...");

    spawn_thread(|| {
        let start_tick = get_ticks();
        sleep(this_thread(), SLEEP_MS);

        // `sleep` only queues the current thread; the syscall/event path then leaves it
        // off the runnable queue. Do the same here so the timer interrupt is what wakes us.
        suspend_to_thread(IDLE.get().unwrap().clone());

        ELAPSED_TICKS.store(get_ticks().saturating_sub(start_tick), Ordering::Release);
        FINISHED.store(true, Ordering::Release);
    });

    while !FINISHED.load(Ordering::Acquire) {}

    let minimum_ticks = SLEEP_MS.saturating_mul(TIMER_HZ).div_ceil(1_000);
    let elapsed_ticks = ELAPSED_TICKS.load(Ordering::Acquire);
    assert!(
        elapsed_ticks >= minimum_ticks,
        "sleep returned after {elapsed_ticks} ticks; expected at least {minimum_ticks}"
    );

    kprintln!("Sleep waited at least {} ms.", SLEEP_MS);
});
