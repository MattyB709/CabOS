use alloc::{
    collections::btree_map::BTreeMap,
    string::{String, ToString},
    sync::{Arc, Weak},
};
use core::sync::atomic::{AtomicUsize, Ordering};

use spin::Once;

use crate::{
    devices::{
        block::BlockDevice,
        char::CharDevice,
        discovery::{BLOCK_DEVICES, CHAR_DEVICES},
    },
    fs::vfs::{Filesystem, FsError, INodeKey, INodeType, VNode},
    sync::{IntMutex, MutexLike},
};

pub static DEV: Once<Arc<Dev>> = Once::new();

pub struct Dev {
    self_ref: Once<Weak<Self>>,
    counter: AtomicUsize,
    devices: IntMutex<BTreeMap<usize, Arc<DevINode>>>,
    root: Once<Arc<DevINode>>,
    fs_id: IntMutex<Option<usize>>,
}

// Router enum to send calls to the correct device type, stored in each DevINode
#[derive(Clone)]
pub enum DeviceBackend {
    Block(Arc<dyn BlockDevice>),
    Char(Arc<dyn CharDevice>),
}

pub struct DevINode {
    fs: Weak<Dev>,
    inumber: usize,
    // The device associated with this inode. For the root this will never be initialized.
    device: Once<DeviceBackend>,
    //Devices can have children!
    children: IntMutex<BTreeMap<String, Arc<DevINode>>>,
}

impl Dev {
    pub fn new() -> Arc<Self> {
        let fs = Arc::new(Self {
            counter: AtomicUsize::new(1),
            devices: IntMutex::new(BTreeMap::new()),
            fs_id: IntMutex::new(None),
            self_ref: Once::new(),
            root: Once::new(),
        });
        fs.self_ref.call_once(|| Arc::downgrade(&fs));
        fs.root.call_once(|| {
            Arc::new(DevINode {
                fs: Arc::downgrade(&fs),
                inumber: 0,
                device: Once::new(),
                children: IntMutex::new(BTreeMap::new()),
            })
        });
        fs
    }

    // TODO add proper path traverasal so not all devices are added to the root
    // TODO also allow for user creation of special /dev files, in general just more flexibility
    pub fn add_device_node(&self, name: &str, device: DeviceBackend) {
        let inumber = self.counter.fetch_add(1, Ordering::SeqCst);
        let root = self.root.get().unwrap();
        let inode = Arc::new(DevINode {
            fs: self.self_ref.get().unwrap().clone(),
            inumber,
            device: Once::new(),
            children: IntMutex::new(BTreeMap::new()),
        });
        inode.device.call_once(|| device);
        self.devices.lock().insert(inumber, inode.clone());
        root.children.lock().insert(name.to_string(), inode);
    }
}

impl Filesystem for Dev {
    fn get_root(&self) -> Result<Arc<dyn VNode>, FsError> {
        self.get_inode(0)
    }

    fn get_inode(&self, inumber: usize) -> Result<Arc<dyn VNode>, FsError> {
        if inumber == 0 {
            return Ok(self.root.get().unwrap().clone());
        }
        if let Some(device) = self.devices.lock().get(&inumber) {
            Ok(device.clone())
        } else {
            Err(FsError::NotFound)
        }
    }

    fn set_filesystem_id(&self, id: Option<usize>) {
        *self.fs_id.lock() = id;
    }

    fn get_filesystem_id(&self) -> Result<usize, FsError> {
        self.fs_id.lock().ok_or(FsError::NotFound)
    }
}

impl VNode for DevINode {
    fn get_inumber(&self) -> usize {
        self.inumber
    }

    fn get_type(&self) -> INodeType {
        match self.device.get() {
            Some(DeviceBackend::Block(_)) => INodeType::Block,
            Some(DeviceBackend::Char(_)) => INodeType::Char,
            None => INodeType::Directory,
        }
    }

    // exact semantics around children in /dev are TBD, currently unused besides children of the root
    fn lookup(&self, name: &str) -> Result<Arc<dyn VNode>, FsError> {
        let children = self.children.lock();
        if let Some(child) = children.get(name) {
            Ok(child.clone())
        } else {
            Err(FsError::NotFound)
        }
    }

    fn read_unaligned(&self, offset: usize, buffer: &mut [u8]) -> Result<usize, FsError> {
        if let Some(device) = self.device.get() {
            return match device {
                DeviceBackend::Block(block) => {
                    block.read(offset, buffer).map_err(|_| FsError::ReadError)
                }
                DeviceBackend::Char(char) => {
                    char.read(buffer, offset).map_err(|_| FsError::ReadError)
                }
            };
        }
        Err(FsError::InvalidOperation)
    }

    fn write_unaligned(&self, offset: usize, buffer: &[u8]) -> Result<usize, FsError> {
        if let Some(device) = self.device.get() {
            return match device {
                DeviceBackend::Block(block) => {
                    block.write(offset, buffer).map_err(|_| FsError::WriteError)
                }
                DeviceBackend::Char(char) => {
                    char.write(buffer, offset).map_err(|_| FsError::WriteError)
                }
            };
        }
        Err(FsError::InvalidOperation)
    }

    fn get_inode_key(&self) -> Result<INodeKey, FsError> {
        let result = INodeKey {
            filesystem_id: self
                .fs
                .upgrade()
                .ok_or(FsError::ReadError)?
                .fs_id
                .lock()
                .ok_or(FsError::ReadError)?,
            inumber: self.inumber,
        };
        Ok(result)
    }

    fn seekable(&self) -> bool {
        match self.device.get() {
            Some(DeviceBackend::Block(_)) => true,
            Some(DeviceBackend::Char(char)) => char.seekable(),
            None => false,
        }
    }
}

// register all character and block devices into devfs based on their desired devfs name, if applicable
// TODO handle devices that want the same name, i.e dev/event0, dev/event1, etc.
pub fn register_devices() {
    let block_devices = BLOCK_DEVICES.lock();
    let dev = DEV.get().unwrap();
    for block in block_devices.iter() {
        if let Some(devfs_name) = block.requested_devfs_name() {
            let device_backend = DeviceBackend::Block(block.clone());
            // this acquires a second lock, lock ordering here is important
            dev.add_device_node(devfs_name, device_backend);
        }
    }

    let char_devices = CHAR_DEVICES.lock();
    for char in char_devices.iter() {
        if let Some(devfs_name) = char.requested_devfs_name() {
            let device_backend = DeviceBackend::Char(char.clone());
            dev.add_device_node(devfs_name, device_backend);
        }
    }
}
