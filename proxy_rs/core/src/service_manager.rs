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
use crate::service_request_manager_webrtc::{service_connection_fib_handler, FibConnectionType};
use crate::structs::{
    gdp_name_to_string, generate_gdp_name_from_string, generate_random_gdp_name,
    get_gdp_name_from_topic, GDPName, GDPPacket, GdpAction, Packet,
};

use crate::service_request_manager_webrtc::{FibChangeAction, FibStateChange};

use redis_async::client;
use redis_async::resp::FromResp;
use serde::{Deserialize, Serialize};

use std::env;
use std::str;
use std::sync::{Arc, Mutex};
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use crate::network::udp::reader_and_writer;
use std::net::{Ipv4Addr, SocketAddr};
use r2r::{Node};
use futures::StreamExt;
use ebpf_routing_manager::NewEbpfTopicRequest; // TODO: replace it out

use tokio::sync::mpsc::{self};

use tokio::time::Duration;


#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize, Hash)]
pub enum TopicManagerAction {
    ADD,
    PAUSE,    // pausing the forwarding of the topic, keeping connections alive
    PAUSEADD, // adding the entry to FIB, but keeps it paused
    RESUME,   // resume a paused topic
    DELETE,   // deleting a local topic interface and all its connections
    RESPONSE,
}


pub struct TopicManagerRequest {
    action: TopicManagerAction,
    topic_name: String,
    topic_type: String,
    certificate: Vec<u8>,
}


// Define your necessary structures and enums (e.g., GDPPacket, FibStateChange, etc.)

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct RosTopicStatus {
    pub action: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct RoutingManagerRequest {
    action: FibChangeAction,
    topic_name: String,
    topic_type: String,
    certificate: Vec<u8>,
    connection_type: Option<String>,
    communication_url: Option<String>,
}

struct RoutingManager {
    ebpf_tx: UnboundedSender<NewEbpfTopicRequest>,
    fib_tx: UnboundedSender<GDPPacket>,
    channel_tx: UnboundedSender<FibStateChange>,
}

impl RoutingManager {
    pub fn new(
        ebpf_tx: UnboundedSender<NewEbpfTopicRequest>,
        fib_tx: UnboundedSender<GDPPacket>,
        channel_tx: UnboundedSender<FibStateChange>,
    ) -> Self {
        Self {
            ebpf_tx,
            fib_tx,
            channel_tx,
        }
    }

    pub async fn handle_sender_routing(&self, mut request_rx: UnboundedReceiver<RoutingManagerRequest>) {
        while let Some(request) = request_rx.recv().await {
            let fib_tx = self.fib_tx.clone();
            let channel_tx = self.channel_tx.clone();
            let ebpf_tx = self.ebpf_tx.clone();
            tokio::spawn(async move {
                let topic_name = request.topic_name.clone();
                let topic_type = request.topic_type.clone();
                let certificate = request.certificate.clone();
                let topic_gdp_name = GDPName(get_gdp_name_from_topic(&topic_name, &topic_type, &certificate));
                let connection_type = match request.connection_type.as_deref() {
                    Some("request") => FibConnectionType::REQUEST,
                    Some("response") => FibConnectionType::RESPONSE,
                    Some("pub") => FibConnectionType::RECEIVER,
                    Some("sub") => FibConnectionType::SENDER,
                    _ => FibConnectionType::BIDIRECTIONAL,
                };
                warn!("sender_network_routing_thread_manager {:?}", connection_type);

                let (local_to_rtc_tx, local_to_rtc_rx) = mpsc::unbounded_channel();
                let _rtc_handle = tokio::spawn(
                    reader_and_writer(
                        topic_gdp_name,
                        format!("{}-{}", request.connection_type.unwrap(), "sender"),
                        fib_tx,
                        ebpf_tx,
                        local_to_rtc_rx,
                    )
                );
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

    pub async fn handle_receiver_routing(&self, mut request_rx: UnboundedReceiver<RoutingManagerRequest>) {
        while let Some(request) = request_rx.recv().await {
            let fib_tx = self.fib_tx.clone();
            let ebpf_tx = self.ebpf_tx.clone();
            tokio::spawn(async move {
                let topic_name = request.topic_name.clone();
                let topic_type = request.topic_type.clone();
                let certificate = request.certificate.clone();
                let topic_gdp_name = GDPName(get_gdp_name_from_topic(&topic_name, &topic_type, &certificate));
                let (_local_to_rtc_tx, local_to_rtc_rx) = mpsc::unbounded_channel();
                let _rtc_handle = tokio::spawn(reader_and_writer(
                    topic_gdp_name,
                    format!("{}-{}", request.connection_type.unwrap(), "receiver"),
                    fib_tx,
                    ebpf_tx,
                    local_to_rtc_rx
                ));
            });
        }
    }
}

struct ROSManager {
    node: Arc<Mutex<r2r::Node>>,
    unique_ros_node_gdp_name: GDPName,
}

impl ROSManager {
    pub fn new(node_name: &str, namespace: &str) -> Self {
        let ctx = r2r::Context::create().expect("context creation failure");
        let node = Arc::new(Mutex::new(Node::create(ctx, node_name, namespace).expect("node creation failure")));
        let unique_ros_node_gdp_name = generate_random_gdp_name();

        let ros_manager_node_clone = node.clone();
        task::spawn_blocking(move || loop {
            std::thread::sleep(Duration::from_millis(100));
            ros_manager_node_clone.lock().unwrap().spin_once(Duration::from_millis(10));
        });

        Self {
            node,
            unique_ros_node_gdp_name,
        }
    }

    pub async fn handle_service_provider(
        &self,
        mut status_recv: UnboundedReceiver<TopicManagerRequest>,
        fib_tx: UnboundedSender<GDPPacket>,
        channel_tx: UnboundedSender<FibStateChange>,
    ) {
        let mut join_handles = vec![];
        let mut existing_topics = vec![];

        while let Some(request) = status_recv.recv().await {
            let topic_name = request.topic_name;
            let topic_type = request.topic_type;
            let action = request.action;
            let certificate = request.certificate;
            let request_tx = fib_tx.clone();
            let topic_gdp_name = GDPName(get_gdp_name_from_topic(&topic_name, &topic_type, &certificate));

            if action != TopicManagerAction::ADD {
                error!("action {:?} not supported in ros_remote_service_provider", action);
                continue;
            }

            let manager_node = self.node.clone();
            info!(
                "[ros_topic_remote_publisher_handler] topic creator for topic {}, type {}, action {:?}",
                topic_name, topic_type, action
            );

            let (ros_tx, mut ros_rx) = mpsc::unbounded_channel();

            if existing_topics.contains(&topic_gdp_name) {
                info!("topic {:?} already exists in existing topics; don't need to create another subscriber", topic_gdp_name);
            } else {
                let channel_update_msg = FibStateChange {
                    action: FibChangeAction::ADD,
                    connection_type: FibConnectionType::RESPONSE,
                    topic_gdp_name: topic_gdp_name,
                    forward_destination: Some(ros_tx),
                    description: Some("ros service response".to_string()),
                };
                let _ = channel_tx.send(channel_update_msg);

                existing_topics.push(topic_gdp_name);
                let mut service = manager_node.lock().unwrap()
                    .create_service_untyped(&topic_name, &topic_type, r2r::QosProfile::default())
                    .expect("topic subscribing failure");

                let ros_handle = tokio::spawn(async move {
                    loop {
                        if let Some(req) = service.next().await {
                            let guid = format!("{:?}", req.request_id);
                            info!("received a ROS request {:?}", guid);
                            let packet_guid = generate_gdp_name_from_string(&guid);
                            let packet = construct_gdp_request_with_guid(topic_gdp_name, self.unique_ros_node_gdp_name.clone(), req.message.clone(), packet_guid);
                            request_tx.send(packet).expect("send for ros subscriber failure");

                            tokio::select! {
                                Some(packet) = ros_rx.recv() => {
                                    info!("received from webrtc in ros_rx {:?}", packet);
                                    let respond_msg = r2r::UntypedServiceSupport::new_from(&topic_type)
                                        .unwrap()
                                        .make_response_msg();
                                    respond_msg.from_binary(packet.payload.unwrap());
                                    info!("the decoded payload to publish is {:?}", respond_msg);
                                    req.respond(respond_msg).expect("could not send service response");
                                },
                                _ = tokio::time::sleep(Duration::from_millis(1000)) => {
                                    error!("timeout for ros_rx");
                                    let respond_msg = r2r::UntypedServiceSupport::new_from(&topic_type)
                                        .unwrap()
                                        .make_response_msg();
                                    req.respond(respond_msg).expect("could not send service response");
                                }
                            }
                        }
                    }
                });

                join_handles.push(ros_handle);
            }
        }
    }

    // Similarly implement the other methods for `ros_local_service_caller`, `ros_topic_remote_publisher`, `ros_topic_remote_subscriber`
}

pub async fn main_service_manager(
    mut service_request_rx: UnboundedReceiver<ROSTopicRequest>
) {
    let (fib_tx, fib_rx) = mpsc::unbounded_channel();
    let (channel_tx, channel_rx) = mpsc::unbounded_channel();

    let ros_manager = ROSManager::new("sgc_remote_service", "namespace");

    let routing_manager = RoutingManager::new(ebpf_tx.clone(), fib_tx.clone(), channel_tx.clone());

    let _service_provider_handle = tokio::spawn(ros_manager.handle_service_provider(service_request_rx, fib_tx.clone(), channel_tx.clone()));

    let _fib_handle = tokio::spawn(async move {
        service_connection_fib_handler(fib_rx, channel_rx).await;
    });

    // Similarly, spawn other necessary tasks for `ros_local_service_caller`, `ros_topic_remote_publisher`, etc.

    let (_sender_routing_handle, sender_routing_rx) = mpsc::unbounded_channel();
    tokio::spawn(routing_manager.handle_sender_routing(sender_routing_rx));

    let (_receiver_routing_handle, receiver_routing_rx) = mpsc::unbounded_channel();
    tokio::spawn(routing_manager.handle_receiver_routing(receiver_routing_rx));

    // Continuously monitor service requests and handle them accordingly.
    loop {
        // Your select! block or any other control flow to keep the main loop running
    }
}