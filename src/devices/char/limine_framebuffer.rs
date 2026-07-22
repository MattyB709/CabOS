use alloc::sync::Arc;

use limine::{framebuffer::Framebuffer, request::FramebufferRequest};

use super::{CharDevice, CharDeviceError};
use crate::{
    devices::{Device, discovery::CHAR_DEVICES},
    sync::{IntMutex, MutexLike},
};

#[used]
#[unsafe(link_section = ".limine_requests")]
pub static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

// made up ioctl request codes for getting the height and pitch of the framebuffer.
const FBIO_GET_HEIGHT: u64 = 0x1;
const FBIO_GET_PITCH: u64 = 0x2;

pub struct LimineFramebuffer<'a> {
    inner: IntMutex<Framebuffer<'a>>,
    length: usize,
}

impl<'a> LimineFramebuffer<'a> {
    pub fn new(framebuffer: Framebuffer<'a>) -> Self {
        let length = framebuffer.height() * framebuffer.pitch();
        Self {
            inner: IntMutex::new(framebuffer),
            length: length as usize,
        }
    }
}

unsafe impl Send for LimineFramebuffer<'_> {}
unsafe impl Sync for LimineFramebuffer<'_> {}

impl<'a> CharDevice for LimineFramebuffer<'a> {
    fn read(&self, buffer: &mut [u8], offset: usize) -> Result<usize, CharDeviceError> {
        // Note: the lock is representative for access to the addr, even though technically what it is guarding is not a mutable reference to the framebuffer.
        // The condition is that the addr will only be accessed when the framebuffer is locked.
        let framebuffer = self.inner.lock();
        let addr = framebuffer.addr();
        // safety: we trust the
        unsafe {
            let framebuffer_slice = core::slice::from_raw_parts(addr as *const u8, self.length);
            let end = usize::min(offset + buffer.len(), framebuffer_slice.len());
            let bytes_to_read = end - offset;
            buffer[..bytes_to_read].copy_from_slice(&framebuffer_slice[offset..end]);
            Ok(bytes_to_read)
        }
    }

    fn write(&self, buffer: &[u8], offset: usize) -> Result<usize, CharDeviceError> {
        let framebuffer = self.inner.lock();
        let addr = framebuffer.addr();
        unsafe {
            let framebuffer_slice = core::slice::from_raw_parts_mut(addr as *mut u8, self.length);
            let end = usize::min(offset + buffer.len(), framebuffer_slice.len());
            let bytes_to_write = end - offset;
            framebuffer_slice[offset..end].copy_from_slice(&buffer[..bytes_to_write]);
            Ok(bytes_to_write)
        }
    }

    fn seekable(&self) -> bool {
        true
    }
}

impl Device for LimineFramebuffer<'_> {
    fn ioctl(&self, request: u64, _arg: u64) -> u64 {
        match request {
            FBIO_GET_HEIGHT => {
                let framebuffer = self.inner.lock();
                framebuffer.height() as u64
            }
            FBIO_GET_PITCH => {
                let framebuffer = self.inner.lock();
                framebuffer.pitch() as u64
            }
            _ => -1i64 as u64,
        }
    }

    fn name(&self) -> &'static str {
        "limine_framebuffer"
    }

    fn requested_devfs_name(&self) -> Option<&'static str> {
        Some("fb")
    }
}

// registers the limine framebuffer as a character device
pub fn register_limine_framebuffer() {
    if let Some(res) = FRAMEBUFFER_REQUEST.get_response()
        && let Some(framebuffer) = res.framebuffers().next()
    {
        let fb = LimineFramebuffer::new(framebuffer);
        let fb_arc = Arc::new(fb);
        CHAR_DEVICES.lock().push(fb_arc);
    }
}
