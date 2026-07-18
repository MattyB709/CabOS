use alloc::string::String;

use crate::devices::Device;
pub mod limine_framebuffer;
#[cfg(target_arch = "x86_64")]
pub mod ps2_kb_m;
pub mod uart_pl011;
pub mod virtio_input;

#[derive(Debug)]
pub enum CharDeviceError {
    ReadError,
    WriteError,
    Other(String),
}

pub trait CharDevice: Device {
    fn read(&self, buffer: &mut [u8], offset: usize) -> Result<usize, CharDeviceError>;

    fn write(&self, buffer: &[u8], offset: usize) -> Result<usize, CharDeviceError>;

    fn seekable(&self) -> bool {
        false
    }
}
