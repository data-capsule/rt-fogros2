extern crate tokio;
extern crate tokio_core;

use crate::api_server::ros_api_server;
use crate::service_manager_udp::ros_service_manager;
use crate::topic_manager::ros_topic_manager;
use futures::future;

use tokio::sync::mpsc;
use utils::error::Result;

use std::net::Ipv4Addr;

use aya::{
    include_bytes_aligned,
    maps::HashMap,
    programs::{tc, SchedClassifier, TcAttachType},
    Bpf,
};
use aya_log::BpfLogger;
use clap::Parser;
use log::{info, warn};
use tokio::signal;

/// inspired by https://stackoverflow.com/questions/71314504/how-do-i-simultaneously-read-messages-from-multiple-tokio-channels-in-a-single-t
/// TODO: later put to another file
#[tokio::main]
async fn router_async_loop() {
    let (topic_request_tx, topic_request_rx) = mpsc::unbounded_channel();

    let (service_request_tx, service_request_rx) = mpsc::unbounded_channel();

    let mut future_handles = Vec::new();

    let ros_topic_manager_handle = tokio::spawn(ros_topic_manager(topic_request_rx));
    future_handles.push(ros_topic_manager_handle);

    let ros_service_manager_handle = tokio::spawn(async move {
        std::thread::sleep(std::time::Duration::from_millis(1000));
        ros_service_manager(service_request_rx).await;
    });

    future_handles.push(ros_service_manager_handle);

    let ros_api_server_handle = tokio::spawn(ros_api_server(topic_request_tx, service_request_tx));
    future_handles.push(ros_api_server_handle);

    future::join_all(future_handles).await;
}

pub async fn ebpf() {

    // This will include your eBPF object file as raw bytes at compile-time and load it at
    // runtime. This approach is recommended for most real-world use cases. If you would
    // like to specify the eBPF program at runtime rather than at compile-time, you can
    // reach for `Bpf::load_file` instead.
    #[cfg(debug_assertions)]
    let mut bpf = Bpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/debug/tc-egress"
    )).unwrap();
    #[cfg(not(debug_assertions))]
    let mut bpf = Bpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/release/tc-egress"
    ));
    if let Err(e) = BpfLogger::init(&mut bpf) {
        // This can happen if you remove all log statements from your eBPF program.
        warn!("failed to initialize eBPF logger: {}", e);
    }
    // error adding clsact to the interface if it is already added is harmless
    // the full cleanup can be done with 'sudo tc qdisc del dev eth0 clsact'.
    let _ = tc::qdisc_add_clsact("ens5");
    let program: &mut SchedClassifier =
        bpf.program_mut("tc_egress").unwrap().try_into().unwrap();
    program.load();
    program.attach("ens5", TcAttachType::Egress);

    // (1)
    let mut blocklist: HashMap<_, u32, u32> =
        HashMap::try_from(bpf.map_mut("BLOCKLIST").unwrap()).unwrap();

    // (2)
    let block_addr: u32 = Ipv4Addr::new(128, 32, 37, 27).try_into().unwrap();

    // (3)
    blocklist.insert(block_addr, 0, 0);

}


/// Show the configuration file
pub fn router() -> Result<()> {
    warn!("router is started!");

    // NOTE: uncomment to use pnet
    // libpnet::pnet_proc_loop();
    router_async_loop();

    Ok(())
}

/// Show the configuration file
pub fn config() -> Result<()> {
    // let config = AppConfig::fetch();
    // info!("{:#}", config);

    Ok(())
}
#[tokio::main]
/// Simulate an error
pub async fn simulate_error() -> Result<()> {
    // let config = AppConfig::fetch().expect("App config unable to load");
    // info!("{:#}", config);
    // test_cert();
    // get address from default gateway

    // ros_sample();
    // TODO: uncomment them
    // webrtc_main("my_id".to_string(), Some("other_id".to_string())).await;
    Ok(())
}
