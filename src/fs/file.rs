use alloc::sync::Arc;

use crate::{
    fs::vfs::{FsError, INodeType, VNode},
    sync::{IntMutex, MutexLike},
};

const SEEK_SET: i32 = 0;
const SEEK_CUR: i32 = 1;
const SEEK_END: i32 = 2;

pub struct File {
    pub vnode: Arc<dyn VNode>,
    pub offset: IntMutex<usize>,
}

impl File {
    pub fn new(vnode: Arc<dyn VNode>) -> Self {
        Self {
            vnode,
            offset: IntMutex::new(0),
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, FsError> {
        let mut offset = self.offset.lock();
        // TODO this is supposed to go through page cache not directly to the filesystem
        let bytes_read = self.vnode.read_unaligned(*offset, buf)?;
        *offset += bytes_read;
        Ok(bytes_read)
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, FsError> {
        let mut offset = self.offset.lock();
        let bytes_written = self.vnode.write_unaligned(*offset, buf)?;
        *offset += bytes_written;
        Ok(bytes_written)
    }

    pub fn seek(&self, offset: i64, whence: i32) -> Result<usize, FsError> {
        if !self.vnode.seekable() {
            return Err(FsError::InvalidOperation);
        }

        let mut current = self.offset.lock();
        let base = match whence {
            SEEK_SET => 0i128,
            SEEK_CUR => *current as i128,
            SEEK_END => self.vnode.size() as i128,
            _ => return Err(FsError::InvalidInput),
        };

        let new_offset = base
            .checked_add(offset as i128)
            .ok_or(FsError::InvalidInput)?;

        if new_offset < 0 || new_offset > usize::MAX as i128 {
            return Err(FsError::InvalidInput);
        }

        *current = new_offset as usize;
        Ok(*current)
    }
}
