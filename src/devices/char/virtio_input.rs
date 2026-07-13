use virtio_drivers::{
    Hal,
    device::input::{InputEvent, VirtIOInput},
    transport::Transport,
};
use zerocopy::IntoBytes;

use super::{CharDevice, CharDeviceError};
use crate::{
    devices::{Device, virtio::VirtioHal},
    sync::{IntMutex, MutexLike},
};

pub struct VirtIOInputDriver<H: Hal, T: Transport> {
    input: IntMutex<VirtIOInput<H, T>>,
}

unsafe impl<T: Transport> Send for VirtIOInputDriver<VirtioHal, T> {}
unsafe impl<T: Transport> Sync for VirtIOInputDriver<VirtioHal, T> {}

impl<T: Transport> VirtIOInputDriver<VirtioHal, T> {
    pub fn new(transport: T) -> Self {
        Self {
            input: IntMutex::new(
                VirtIOInput::new(transport).expect("failed to initialize VirtIO input device"),
            ),
        }
    }
}

impl<T: Transport> CharDevice for VirtIOInputDriver<VirtioHal, T> {
    fn read(&self, buffer: &mut [u8]) -> Result<usize, CharDeviceError> {
        let mut bytes_read = 0;
        let len = buffer.len();
        let mut input = self.input.lock();
        let event_size = core::mem::size_of::<InputEvent>();

        while bytes_read + event_size <= len
            && let Some(event) = input.pop_pending_event()
        {
            let event_bytes = event.as_bytes();
            buffer[bytes_read..bytes_read + event_size].copy_from_slice(event_bytes);
            bytes_read += event_size;
        }
        Ok(bytes_read)
    }

    fn write(&self, _buffer: &[u8]) -> Result<usize, CharDeviceError> {
        Err(CharDeviceError::WriteError)
    }
}

impl<T: Transport> Device for VirtIOInputDriver<VirtioHal, T> {
    // ioctl not registered yet
    fn ioctl(&self, _request: u64, _arg1: u64, _arg2: u64) -> u64 {
        0
    }

    fn name(&self) -> &'static str {
        "virtio-input"
    }

    fn requested_devfs_name(&self) -> Option<&'static str> {
        Some("event")
    }
}
