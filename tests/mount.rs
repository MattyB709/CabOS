#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel_common::test_runner)]

kernel_common::integration_test!({
    extern crate alloc;

    use kernel_common::{
        devices::discovery::BLOCK_DEVICES,
        fs::{
            ext2::Ext2,
            vfs::{VFS, traverse_path},
        },
        sync::MutexLike,
    };
    let mut block_devices = BLOCK_DEVICES.lock();
    let ext2 = Ext2::new_from_block_devices(&mut block_devices)
        .expect("ext2 filesystem not found on attached block devices");
    drop(block_devices);

    VFS.set_root(ext2.clone()).unwrap();
    let root = VFS.get_root().unwrap();
    let cat = root.lookup("cat").unwrap();
    VFS.mount(cat, ext2).unwrap();
    let hello = traverse_path(root, "/cat/hello.txt").unwrap();
    let mut buffer = alloc::vec![0u8; 1024];
    hello.read_unaligned(0, &mut buffer).unwrap();
    assert!(buffer[0] == b'e');
});
