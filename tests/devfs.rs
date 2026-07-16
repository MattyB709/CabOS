#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel_common::test_runner)]

use kernel_common::devices::Device;

kernel_common::integration_test!({
    extern crate alloc;

    use alloc::sync::Arc;

    use kernel_common::{
        devices::{
            char::{CharDevice, CharDeviceError},
            discovery::BLOCK_DEVICES,
        },
        fs::{
            dev::{DEV, DeviceBackend},
            ext2::Ext2,
            vfs::VFS,
        },
        print::kprintln,
        sync::MutexLike,
    };

    // Trivial device: reads zeros, silently accepts writes.
    struct NullDevice;

    impl CharDevice for NullDevice {
        fn read(&self, buffer: &mut [u8], _offset: usize) -> Result<usize, CharDeviceError> {
            buffer.fill(0);
            Ok(buffer.len())
        }

        fn write(&self, buffer: &[u8], _offset: usize) -> Result<usize, CharDeviceError> {
            Ok(buffer.len())
        }
    }

    impl Device for NullDevice {
        fn ioctl(&self, _request: u64, _arg1: u64) -> u64 {
            0
        }

        fn name(&self) -> &'static str {
            "null"
        }

        fn requested_devfs_name(&self) -> Option<&'static str> {
            Some("null")
        }
    }

    let mut block_devices = BLOCK_DEVICES.lock();
    let ext2 = Ext2::new_from_block_devices(&mut block_devices)
        .expect("ext2 filesystem not found on attached block devices");
    drop(block_devices);

    let _ = VFS.mount(ext2.clone(), &["/"]).unwrap();

    // Register the device at /dev/null_test.
    let dev = DEV.get().expect("DEV not initialized");
    let null = Arc::new(NullDevice);
    dev.add_device_node(null.name(), DeviceBackend::Char(null));
    kprintln!("registered /dev/null");

    // Reach /dev via VFS mount traversal, then look up the device inode by name.
    let root = VFS.get_root().expect("VFS root not set");
    let dev_root = VFS
        .partial_lookup(&root, &["/", "dev"])
        .expect("mount traversal to /dev failed");
    let null_node = dev_root.lookup("null").expect("/dev/null not found");
    kprintln!("found /dev/null via VFS");

    // Read: device should fill the buffer with zeros.
    let mut buf = alloc::vec![0xFFu8; 8];
    let n = null_node
        .read_unaligned(0, &mut buf)
        .expect("read_unaligned failed");
    assert_eq!(n, 8, "expected 8 bytes read");
    assert!(
        buf.iter().all(|&b| b == 0),
        "expected all-zero bytes from NullDevice"
    );
    kprintln!("read check passed");

    // Write: device should accept the write and report the correct byte count.
    let payload = b"hello devfs";
    let written = null_node
        .write_unaligned(0, payload)
        .expect("write_unaligned failed");
    assert_eq!(written, payload.len(), "unexpected write byte count");
    kprintln!("write check passed");

    kprintln!("devfs integration test passed");
});
