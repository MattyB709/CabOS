// ideally this would be a hash map, but we'd need another external library or in-house impl for that
use alloc::{collections::btree_map::BTreeMap, string::String, sync::Arc};
use core::sync::atomic::{AtomicUsize, Ordering};

use spin::Once;

use crate::{
    memory::virtual_memory_2::MapBacking,
    sync::{IntMutex, MutexLike},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsError {
    PathMalformed,
    NotFound,
    AlreadyExists,
    NoSpace,
    WriteError,
    ReadError,
    InvalidInput,
    InvalidOperation,
    NotImplemented,
    Corrupted(String),
    Other(String),
}

pub struct VFS {
    filesystems: IntMutex<BTreeMap<usize, Arc<dyn Filesystem>>>,
    filesystem_id_counter: AtomicUsize,
    mount_points: IntMutex<BTreeMap<INodeKey, Arc<dyn VNode>>>,
    reverse_mount_points: IntMutex<BTreeMap<INodeKey, Arc<dyn VNode>>>, // used to map from a mountpoint to its underlying vnode
    root: Once<Arc<dyn VNode>>,
}

pub static VFS: VFS = VFS {
    filesystems: IntMutex::new(BTreeMap::new()),
    filesystem_id_counter: AtomicUsize::new(0),
    mount_points: IntMutex::new(BTreeMap::new()),
    reverse_mount_points: IntMutex::new(BTreeMap::new()),
    root: Once::new(),
};

impl VFS {
    fn register_filesystem(&self, fs: &Arc<dyn Filesystem>) -> usize {
        let mut filesystems = self.filesystems.lock();
        let id = match fs.get_filesystem_id() {
            Ok(id) => id,
            Err(_) => {
                let id = self.filesystem_id_counter.fetch_add(1, Ordering::SeqCst);
                fs.set_filesystem_id(Some(id));
                id
            }
        };
        filesystems.entry(id).or_insert_with(|| fs.clone());
        id
    }

    pub fn get_inode(&self, key: &INodeKey) -> Result<Arc<dyn VNode>, FsError> {
        let inode = self
            .filesystems
            .lock()
            .get(&key.filesystem_id)
            .ok_or(FsError::NotFound)?
            .get_inode(key.inumber)?;
        Ok(inode)
    }

    pub fn set_root(&self, fs: Arc<dyn Filesystem>) -> Result<(), FsError> {
        self.register_filesystem(&fs);
        let root_fs = fs.get_root()?;
        self.root.call_once(|| root_fs.clone());
        Ok(())
    }

    pub fn get_root(&self) -> Option<Arc<dyn VNode>> {
        self.root.get().cloned()
    }

    pub fn mount(
        &self,
        mountpoint: Arc<dyn VNode>,
        fs: Arc<dyn Filesystem>,
    ) -> Result<(), FsError> {
        if mountpoint.get_type() != INodeType::Directory {
            return Err(FsError::InvalidOperation);
        }

        self.register_filesystem(&fs);

        let mountpoint_key = mountpoint.get_inode_key()?;
        let mount_root = fs.get_root()?;

        // this insert replaces if anything was there. We ignore it under the assumption the kernel
        // will not try to mount twice on the same underlying vnode, and userland will not have access
        // to the underlying mountpoint
        self.mount_points
            .lock()
            .insert(mountpoint_key, mount_root.clone());
        // TODO inserting a mount should be atomic wrt both mount and reverse mount maps
        self.reverse_mount_points
            .lock()
            .insert(mount_root.get_inode_key()?, mountpoint); 
        Ok(())
    }

    // `mountpoint` is the vnode in the parent filesystem that is covered by the mount.
    pub fn unmount(&self, mountpoint: Arc<dyn VNode>) -> Result<(), FsError> {
        let mountpoint_key = mountpoint.get_inode_key()?;
        let mut mount_points = self.mount_points.lock();
        let mount_root = mount_points
            .get(&mountpoint_key)
            .cloned()
            .ok_or(FsError::NotFound)?;
        let mount_root_key = mount_root.get_inode_key()?;

        let mut reverse_mount_points = self.reverse_mount_points.lock();
        if !reverse_mount_points.contains_key(&mount_root_key) {
            return Err(FsError::Corrupted(
                "mount point is missing its reverse mapping".into(),
            ));
        }

        mount_points.remove(&mountpoint_key);
        reverse_mount_points.remove(&mount_root_key);
        Ok(())
    }
}

pub fn traverse_path(start_node: Arc<dyn VNode>, path: &str) -> Result<Arc<dyn VNode>, FsError> {
    if path.is_empty() {
        return Err(FsError::PathMalformed);
    }
    let components = path.split('/').filter(|s| !s.is_empty());
    let start_mount = VFS
        .mount_points
        .lock()
        .get(&start_node.get_inode_key()?)
        .cloned();
    let mut node = start_mount.unwrap_or(start_node);

    for component in components {
        if component == "." {
            continue;
        } else if component == ".." {
            let key = node.get_inode_key()?;
            node = match VFS.reverse_mount_points.lock().get(&key).cloned() {
                Some(mount) => mount.lookup(component)?,
                None => node.lookup(component)?,
            }
        } else {
            let child = node.lookup(component)?;
            let key = child.get_inode_key()?;
            node = match VFS.mount_points.lock().get(&key).cloned() {
                Some(mount) => mount,
                None => child,
            }
        }
    }

    Ok(node)
}

// general path for traversing a path, including mountpoints. Returns both the VNode and its parent.

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug)]
pub struct INodeKey {
    pub filesystem_id: usize,
    pub inumber: usize,
}

impl INodeKey {
    pub fn new(filesystem_id: usize, inumber: usize) -> Self {
        Self {
            filesystem_id,
            inumber,
        }
    }

    pub fn get_inode(&self) -> Result<Arc<dyn VNode>, FsError> {
        VFS.get_inode(self)
    }
}

pub trait Filesystem: Send + Sync {
    fn get_root(&self) -> Result<Arc<dyn VNode>, FsError>;
    fn get_inode(&self, inumber: usize) -> Result<Arc<dyn VNode>, FsError>;

    // these are for the VFS to get id's from the filesystem, particularly so a vnode's fs id can be recovered easily
    fn set_filesystem_id(&self, id: Option<usize>);
    fn get_filesystem_id(&self) -> Result<usize, FsError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum INodeType {
    File,
    Directory,
    Char,
    Block,
    // symlink possibly
    Other,
}

// dyn inode works as a typical vnode
pub trait VNode: Send + Sync {
    // Files
    fn get_inumber(&self) -> usize;

    fn get_type(&self) -> INodeType;

    // add default implementations for all these types so that filesystems don't need to
    // implement unnecessary functions, if they're a directory they just implement directory functions, etc
    fn read_page(&self, _physical_address: usize, _offset: usize) -> Result<usize, FsError> {
        Err(FsError::NotImplemented)
    }

    fn write_page(&self, _physical_address: usize, _offset: usize) -> Result<usize, FsError> {
        Err(FsError::NotImplemented)
    }
    // Directory
    fn lookup(&self, _target: &str) -> Result<Arc<dyn VNode>, FsError> {
        Err(FsError::NotImplemented)
    }

    fn add_entry(
        &self,
        _target: &str,
        _inumber: usize, // TODO this shouldn't take inumber
        _inode_type: INodeType,
    ) -> Result<(), FsError> {
        Err(FsError::NotImplemented)
    }

    // should only be implemented for directories
    fn create_child(&self, _name: &str, _inode_type: INodeType) -> Result<Arc<dyn VNode>, FsError> {
        Err(FsError::NotImplemented)
    }

    // file size, can be undefined
    fn size(&self) -> usize {
        0
    }

    // note: these really are not the main interface for vfs, reads and writes should be done through page cache,
    // this is for non-caching and convenience.
    fn read_unaligned(&self, _offset: usize, _buffer: &mut [u8]) -> Result<usize, FsError> {
        Err(FsError::NotImplemented)
    }

    fn write_unaligned(&self, _offset: usize, _buffer: &[u8]) -> Result<usize, FsError> {
        Err(FsError::NotImplemented)
    }

    // page cache needs to know filesystem id for InodeKey, this provides a way to get it. An Inode should store a reference to
    // whatever fs it's on.
    // TODO this should probably be reworked to never throw an error
    fn get_inode_key(&self) -> Result<INodeKey, FsError> {
        Err(FsError::NotImplemented)
    }

    fn ioctl(&self, _request: u64, _arg: u64) -> Result<u64, FsError> {
        Err(FsError::NotImplemented)
    }

    // this is overridden for special filesystems that have more complicated seek semantics, like /dev
    fn seekable(&self) -> bool {
        matches!(self.get_type(), INodeType::File | INodeType::Block)
    }

    // used by devices and fake filesystems to configure mmap behavior
    // if a file is not able to be mmaped, it should overwrite this and return InvalidOperation
    fn prepare_mmap(&self, _offset: usize) -> Result<MapBacking, FsError> {
        Err(FsError::NotImplemented)
    }
    // Symlink
    // fn traverse() -> str
}
