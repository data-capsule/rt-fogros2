use crate::api_server::ROSTopicRequest;

use crate::db::{
    add_entity_to_database_as_transaction, allow_keyspace_notification, get_entity_from_database,
    get_redis_address_and_port, get_redis_url,
};
#[cfg(feature = "ros")]
use crate::network::webrtc::{register_webrtc_stream, webrtc_reader_and_writer};

use crate::pipeline::{
    construct_gdp_forward_from_bytes, construct_gdp_request_with_guid,
    construct_gdp_response_with_guid,
};
use crate::service_request_manager_webrtc::{service_connection_fib_handler};
use fogrs_common::packet_structs::{
    gdp_name_to_string, generate_gdp_name_from_string, generate_random_gdp_name,
    get_gdp_name_from_topic, GDPName, GDPPacket, GdpAction, Packet,
};
use fogrs_common::fib_structs::RoutingManagerRequest;
use fogrs_common::fib_structs::{FibChangeAction, FibStateChange, FibConnectionType};

use redis_async::client;
use redis_async::resp::FromResp;
use serde::{Deserialize, Serialize};

use crate::ebpf_routing_manager::NewEbpfTopicRequest;
use crate::network::udp::reader_and_writer;
use futures::StreamExt;
use std::env;
use std::net::{Ipv4Addr, SocketAddr};
use std::str;
use std::sync::{Arc, Mutex};
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender}; // TODO: replace it out

use tokio::sync::mpsc::{self};

use tokio::time::Duration;

use fogrs_ros::ROSManager;

struct RoutingManager {
    // ebpf_tx: UnboundedSender<NewEbpfTopicRequest>,
    fib_tx: UnboundedSender<GDPPacket>,
    channel_tx: UnboundedSender<FibStateChange>,
}

impl RoutingManager {
    pub fn new(
        // ebpf_tx: UnboundedSender<NewEbpfTopicRequest>,
        fib_tx: UnboundedSender<GDPPacket>,
        channel_tx: UnboundedSender<FibStateChange>,
    ) -> Self {
        Self {
            // ebpf_tx,
            fib_tx,
            channel_tx,
        }
    }

    pub async fn handle_sender_routing(
        &self, mut request_rx: UnboundedReceiver<RoutingManagerRequest>,
    ) {
        while let Some(request) = request_rx.recv().await {
            let fib_tx = self.fib_tx.clone();
            let channel_tx = self.channel_tx.clone();
            // let ebpf_tx = self.ebpf_tx.clone();
            tokio::spawn(async move {
                let topic_name = request.topic_name.clone();
                let topic_type = request.topic_type.clone();
                let certificate = request.certificate.clone();
                let topic_gdp_name = GDPName(get_gdp_name_from_topic(
                    &topic_name,
                    &topic_type,
                    &certificate,
                ));
                let connection_type = match request.connection_type.as_deref() {
                    Some("request") => FibConnectionType::REQUEST,
                    Some("response") => FibConnectionType::RESPONSE,
                    Some("pub") => FibConnectionType::RECEIVER,
                    Some("sub") => FibConnectionType::SENDER,
                    _ => FibConnectionType::BIDIRECTIONAL,
                };
                warn!(
                    "sender_network_routing_thread_manager {:?}",
                    connection_type
                );

                let (local_to_rtc_tx, local_to_rtc_rx) = mpsc::unbounded_channel();
                let _rtc_handle = tokio::spawn(reader_and_writer(
                    topic_gdp_name,
                    format!("{}-{}", request.connection_type.unwrap(), "sender"),
                    fib_tx,
                    // ebpf_tx,
                    local_to_rtc_rx,
                ));
                let channel_update_msg = FibStateChange {
                    action: FibChangeAction::ADD,
                    topic_gdp_name: topic_gdp_name,
                    connection_type: connection_type,
                    forward_destination: Some(local_to_rtc_tx),
                    description: Some(format!(
                        "udp stream for topic_name {}, topic_type {}, connection_type {:?}",
                        topic_name, topic_type, connection_type
                    )),
                };
                let _ = channel_tx.send(channel_update_msg);
                info!("remote sender sent channel update message");
            });
        }
    }

    pub async fn handle_receiver_routing(
        &self, mut request_rx: UnboundedReceiver<RoutingManagerRequest>,
    ) {
        while let Some(request) = request_rx.recv().await {
            let fib_tx = self.fib_tx.clone();
            // let ebpf_tx = self.ebpf_tx.clone();
            tokio::spawn(async move {
                let topic_name = request.topic_name.clone();
                let topic_type = request.topic_type.clone();
                let certificate = request.certificate.clone();
                let topic_gdp_name = GDPName(get_gdp_name_from_topic(
                    &topic_name,
                    &topic_type,
                    &certificate,
                ));
                let (_local_to_rtc_tx, local_to_rtc_rx) = mpsc::unbounded_channel();
                let _rtc_handle = tokio::spawn(reader_and_writer(
                    topic_gdp_name,
                    format!("{}-{}", request.connection_type.unwrap(), "receiver"),
                    fib_tx,
                    // ebpf_tx,
                    local_to_rtc_rx,
                ));
            });
        }
    }
}

pub async fn main_service_manager(mut service_request_rx: UnboundedReceiver<ROSTopicRequest>) {
    let (fib_tx, fib_rx) = mpsc::unbounded_channel();
    let (channel_tx, channel_rx) = mpsc::unbounded_channel();

    let ros_manager = ROSManager::new("sgc_remote_service", "namespace");

    let routing_manager = RoutingManager::new(
        // ebpf_tx.clone(),
        fib_tx.clone(),
        channel_tx.clone(),
    );
    
    // let _service_provider_handle = tokio::spawn(ros_manager.handle_service_provider(service_request_rx, fib_tx.clone(), channel_tx.clone()));

    // let _service_provider_handle = tokio::spawn(ROSManager::handle_service_provider(service_request_rx, fib_tx.clone(), channel_tx.clone()));

    let _fib_handle = tokio::spawn(async move {
        service_connection_fib_handler(fib_rx, channel_rx).await;
    });

    // Similarly, spawn other necessary tasks for `ros_local_service_caller`, `ros_topic_remote_publisher`, etc.

    // let (_sender_routing_handle, sender_routing_rx) = mpsc::unbounded_channel();
    // tokio::spawn(routing_manager.handle_sender_routing(sender_routing_rx));

    // let (_receiver_routing_handle, receiver_routing_rx) = mpsc::unbounded_channel();
    // tokio::spawn(routing_manager.handle_receiver_routing(receiver_routing_rx));

    // Continuously monitor service requests and handle them accordingly.
    loop {
        // Your select! block or any other control flow to keep the main loop running
    }
}
