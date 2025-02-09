#[macro_use] extern crate log;

// sub crates and primitives
pub mod api_server;
pub mod crypto;
pub mod network;
pub mod rib;

// network processing
// pub mod connection_fib;
pub mod db;

// util
pub mod commands;
pub mod logger;
// pub mod service_manager_udp;
pub mod service_request_manager;
// pub mod service_manager_webrtc;
pub mod routing_manager;
pub mod service_manager;
// pub mod service_request_manager_webrtc;
// pub mod topic_manager;
// pub mod ebpf_routing_manager;
pub mod util;
use fogrs_utils::error::Result;
pub fn start() -> Result<()> {
    // does nothing

    Ok(())
}
