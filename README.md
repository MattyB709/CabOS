# CabOS

CabOS is a multicore operating system kernel written in Rust. It targets both x86-64 and AArch64 and currently includes userspace processes, virtual memory, an ext2-backed VFS, and a device discovery framework for device tree, ACPI tables, and PCI. 

## Requirements

The repository includes a Cargo alias named `buildtool` that builds the kernel, creates boot and filesystem images, and launches QEMU. You will need:

- A recent nightly Rust toolchain
- QEMU for the target architecture:
  - `qemu-system-x86_64`
  - `qemu-system-aarch64`
- `mkfs.ext2`
- `strip` for x86-64 builds
- `aarch64-linux-gnu-strip` for AArch64 builds
- `rust-gdb` for debugging

The buildtool downloads the required Limine EFI executable and OVMF firmware on first use, so the first build requires network access.

Run all commands below from the repository root.

## Quick start

Run CabOS on x86-64 with the default filesystem directory:

```sh
cargo buildtool qemu
```

Run it on AArch64:

```sh
cargo buildtool qemu --target aarch64
```

Serial output is written to:

```text
run/serial.txt
```

QEMU diagnostic output is written to:

```text
run/qemu.log
```

Use `cargo buildtool help` or `cargo buildtool <command> --help` for the complete command-line reference.

## Buildtool commands

### Build a bootable image

```sh
cargo buildtool image
cargo buildtool image --target aarch64
cargo buildtool image --release
```

The default target is x86-64.

### Run in QEMU

```sh
cargo buildtool qemu
```

Useful options include:

```sh
cargo buildtool qemu --target aarch64
cargo buildtool qemu --cores 4
cargo buildtool qemu --mem 8
cargo buildtool qemu --release
cargo buildtool qemu --kvm
cargo buildtool qemu --filesystem-path path/to/directory
```

Short forms are also available:

```text
-t, --target
-j, --cores
-m, --mem
-r, --release
-k, --kvm
-f, --filesystem-path
```

`--mem` is specified in GiB. KVM is host- and architecture-dependent; use it only when the host supports acceleration for the selected target.

The directory passed through `--filesystem-path` is packed into a 64 MiB ext2 image and attached to QEMU as a virtio block device. The default is:

```text
fs_path/
```

Generated filesystem images are cached in `buildtool-cache/`. Writes made by CabOS modify the cached image and may persist across runs. If filesystem state becomes stale or corrupted, rebuild it with:

```sh
cargo buildtool clean
```

This removes the complete buildtool cache, including downloaded/generated artifacts. To reset only the normal QEMU filesystem, remove the matching `buildtool-cache/fs_img-*.ext2` file.

### Run tests

Run all configured tests for one architecture:

```sh
cargo buildtool test
cargo buildtool test --target aarch64
cargo buildtool test --release
```

Run one test configuration:

```sh
cargo buildtool qemu-test test_cfgs/virtual_memory/virtual_memory_x86_64_test.json
```

Stream serial output live and bypass cached test results:

```sh
cargo buildtool qemu-test test_cfgs/virtual_memory/virtual_memory_x86_64_test.json --stdout
```

Integration-test configurations are stored under `test_cfgs/`. A configuration selects the test binary, architecture, expected serial output, QEMU arguments, timeout, and optional source directory for its ext2 filesystem.

### Debug with GDB

The QEMU command starts a GDB server on port `1234`. Start QEMU in one terminal, then run the matching GDB command in another:

```sh
cargo buildtool gdb
```

For AArch64:

```sh
cargo buildtool gdb --target aarch64
```

When QEMU is started with `--kvm`, pass `--kvm` to the GDB command as well:

```sh
cargo buildtool gdb --kvm
```

### Clean generated files

```sh
cargo buildtool clean
```

## Display modes: Doom versus flanterm

CabOS currently has two users of the boot framebuffer:

1. The kernel can use it for text logging through flanterm.
2. Userspace can access it through the `/dev/fb` device, which is used by the current Doom setup.

The mode is selected through the kernel command line in `resources/limine.conf`.

### Run Doom

For Doom, framebuffer logging must be disabled so the framebuffer is registered as `/dev/fb` for userspace:

```text
cmdline: logging: { serial: { enable: true }, fb: { enable: false } }
```

The default `fs_path/` currently contains the userspace files used by the Doom boot path, including:

```text
fs_path/doomgeneric-cabos
fs_path/DOOM1.WAD
```

Then run:

```sh
cargo buildtool qemu
```

The current non-test kernel entry path in `usual_main` loads `doomgeneric-cabos` from the ext2 root and passes `./DOOM1.WAD` as an argument. This is for demoing purposes, not a final userspace boot path. The DOOM executable was compiled using an mlibc port to CabOS.

### Use flanterm for kernel display output

To let the kernel use the framebuffer as a text console, enable framebuffer logging:

```text
cmdline: logging: { serial: { enable: true }, fb: { enable: true } }
```

When framebuffer logging is enabled, CabOS does not register the Limine framebuffer as `/dev/fb`. This prevents userspace and flanterm from independently treating the same framebuffer as exclusively owned display memory.

Serial logging can be independently enabled or disabled with the `serial.enable` field. See [`docs/CMDLINE.md`](docs/CMDLINE.md) for the complete command-line syntax.

## Architectures

CabOS currently supports:

- x86-64 (`x86_64-unknown-none`)
- AArch64 (`aarch64-unknown-none`)

The buildtool uses QEMU's `q35` machine with a standard VGA device on x86-64. On AArch64 it uses the `virt` machine with a RAM framebuffer and a virtio keyboard.

Some features and tests work on one architecture but have not yet been implemented on the other, such as userprocess stack setup on X86-64

## Repository layout

```text
src/              Kernel source
buildtool/        Image building, QEMU, GDB, and test runner
proc-macros/      Project procedural macros
flanterm/         Framebuffer terminal integration
resources/        Limine configuration and boot resources
fs_path/          Default directory packed into the runtime ext2 image
tests/            Kernel integration-test binaries
test_cfgs/        QEMU integration-test configurations and fixtures
docs/             Design and subsystem documentation
run/              Serial and QEMU logs from the latest normal run
buildtool-cache/  Generated and downloaded build artifacts
```

## Further documentation

Additional subsystem documentation is available in [`docs/`](docs/), including notes on virtual memory, synchronization, threading, symbols, command-line parsing, and other kernel internals.
