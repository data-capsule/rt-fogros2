#![no_std]

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PacketLog {
    pub ipv4_address: u32,
    pub action: i32,
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for PacketLog {}
