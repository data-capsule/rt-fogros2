extern crate tokio;

use std::sync::Arc;

// use crate::ebpf_routing_manager::ebpf_routing_manager;
use crate::api_server::ros_api_server;
// use crate::_manager::ros_topic_manager;
use futures::future;

use fogrs_utils::error::Result;
use tokio::sync::mpsc;
use fogrs_signaling::Server;


/// TODO: later put to another file
#[tokio::main]
async fn webrtc_router_async_loop() {
    // console_subscriber::init();
    env_logger::builder().format_timestamp_micros().init();
    let (topic_request_tx, topic_request_rx) = mpsc::unbounded_channel();

    let (service_request_tx, service_request_rx) = mpsc::unbounded_channel();

    let mut future_handles = Vec::new();

    // let ros_topic_manager_handle = tokio::spawn(ros_topic_manager(topic_request_rx));
    // future_handles.push(ros_topic_manager_handle);

    let ros_service_manager_handle = tokio::spawn(async move {
        std::thread::sleep(std::time::Duration::from_millis(100));
        crate::service_manager::main_service_manager(service_request_rx).await;
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



#[tokio::main]
async fn run_async_signaling_server() {

    let server = Arc::new(Server::new());
    server.run("127.0.0.1:8080").await;

}

/// Show the configuration file
pub fn config() -> Result<()> {
    run_async_signaling_server();
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
