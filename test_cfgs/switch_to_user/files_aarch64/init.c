static inline long syscall_1_arg(long syscall_no, long arg0) {
    register long x8 __asm__("x8") = syscall_no;
    register long x0 __asm__("x0") = arg0;

    __asm__ volatile(
        "svc #0"
        : "+r"(x0)
        : "r"(x8)
        : "memory"
    );

    return x0;
}

static inline long syscall_0_arg(long syscall_no) {
    register long x8 __asm__("x8") = syscall_no;
    register long x0 __asm__("x0");

    __asm__ volatile(
        "svc #0"
        : "+r"(x0)
        : "r"(x8)
        : "memory"
    );

    return x0;
}

static inline long syscall_3_arg(long syscall_no, long arg0, long arg1, long arg2) {
    register long x8 __asm__("x8") = syscall_no;
    register long x0 __asm__("x0") = arg0;
    register long x1 __asm__("x1") = arg1;
    register long x2 __asm__("x2") = arg2;

    __asm__ volatile(
        "svc #0"
        : "+r"(x0)
        : "r"(x8), "r"(x1), "r"(x2)
        : "memory"
    );

    return x0;
}

void start(void) {

  // get pid, which should be 1 as in this test it is the only process in the system
  long result = syscall_0_arg(172);
  if (result == 1) {
    syscall_1_arg(93, 0);
  } else {
    // exit with code 0
    syscall_1_arg(93, 1);
  }

  char buf[2];
  buf[0] = 'x';
  buf[1] = '\0';

  // write syscall: write(fd=1, buf, count=1)
  syscall_3_arg(64, 1, (long)buf, 1);

}
