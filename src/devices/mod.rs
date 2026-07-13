pub mod block;
pub mod char;
pub mod discovery;
pub mod network;
pub mod virtio;

pub trait Device: Send + Sync {
    fn ioctl(&self, request: u64, arg1: u64, arg2: u64) -> u64;

    fn name(&self) -> &'static str;

    // Returns the name of the device in devfs if it is meant to be registered there
    fn requested_devfs_name(&self) -> Option<&'static str> {
        None
    }
}
