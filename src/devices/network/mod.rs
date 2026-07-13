use alloc::string::String;
pub mod virtio_net;

use crate::devices::Device;

#[derive(Debug)]
pub enum NetworkError {
    SendError,
    ReceiveError,
    BufferTooSmall,
    Other(String),
}

pub trait NetworkDevice: Device {
    fn send_packet(&self, packet: &[u8]) -> Result<(), NetworkError>;
    // returns the number of bytes written into buffer
    fn receive_packet(&self, buffer: &mut [u8]) -> Result<usize, NetworkError>;
}
