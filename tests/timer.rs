#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel_common::test_runner)]

kernel_common::integration_test!({
    use kernel_common::{arch::get_ticks, print::kprintln};

    kprintln!("Testing timer...");
    // kprintln!("Timer is ticking!");

    let initial_ticks = get_ticks();

    for _ in 0..10_000_000 {
        unsafe { core::arch::asm!("nop") };
    }

    let final_ticks = get_ticks();

    if final_ticks > initial_ticks {
        kprintln!("Timer is ticking!");
    } else {
        kprintln!("Timer does not appear to be ticking.");
    }
});
