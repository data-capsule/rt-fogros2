extern crate tokio;
extern crate tokio_core;

// use crate::ebpf_routing_manager::ebpf_routing_manager;
use crate::{api_server::ros_api_server, util::get_non_existent_ip_addr};
use crate::topic_manager::ros_topic_manager;
use futures::future;

use tokio::sync::mpsc;
use utils::error::Result;


/// inspired by https://stackoverflow.com/questions/71314504/how-do-i-simultaneously-read-messages-from-multiple-tokio-channels-in-a-single-t

// #[tokio::main]
// async fn ebpf_router_async_loop() {
//     let (topic_request_tx, topic_request_rx) = mpsc::unbounded_channel();

//     let (service_request_tx, service_request_rx) = mpsc::unbounded_channel();

//     let (ebpf_request_tx, ebpf_request_rx) = mpsc::unbounded_channel();

//     let mut future_handles = Vec::new();

//     let ros_topic_manager_handle = tokio::spawn(ros_topic_manager(topic_request_rx));
//     future_handles.push(ros_topic_manager_handle);

//     let ros_service_manager_handle = tokio::spawn(async move {
//         std::thread::sleep(std::time::Duration::from_millis(1000));
//         crate::service_manager_udp::ros_service_manager(ebpf_request_tx, service_request_rx).await;
//     });

//     future_handles.push(tokio::spawn(ebpf_routing_manager(ebpf_request_rx)));
//     future_handles.push(ros_service_manager_handle);

//     let ros_api_server_handle = tokio::spawn(ros_api_server(topic_request_tx, service_request_tx));
//     future_handles.push(ros_api_server_handle);

//     future::join_all(future_handles).await;
// }


/// inspired by https://stackoverflow.com/questions/71314504/how-do-i-simultaneously-read-messages-from-multiple-tokio-channels-in-a-single-t
/// TODO: later put to another file
#[tokio::main]
async fn webrtc_router_async_loop() {
    let (topic_request_tx, topic_request_rx) = mpsc::unbounded_channel();

    let (service_request_tx, service_request_rx) = mpsc::unbounded_channel();

    let mut future_handles = Vec::new();

    let ros_topic_manager_handle = tokio::spawn(ros_topic_manager(topic_request_rx));
    future_handles.push(ros_topic_manager_handle);

    let ros_service_manager_handle = tokio::spawn(async move {
        std::thread::sleep(std::time::Duration::from_millis(1000));
        crate::service_manager_webrtc::ros_service_manager(service_request_rx).await;
    });

    future_handles.push(ros_service_manager_handle);

    let ros_api_server_handle = tokio::spawn(ros_api_server(topic_request_tx, service_request_tx));
    future_handles.push(ros_api_server_handle);

    future::join_all(future_handles).await;
}


/// Show the configuration file
pub fn router() -> Result<()> {
    warn!("router is started!");

    // NOTE: uncomment to use pnet
    // libpnet::pnet_proc_loop();
    webrtc_router_async_loop();

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
