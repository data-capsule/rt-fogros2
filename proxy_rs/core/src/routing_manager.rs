
use crate::network::udp::{get_socket_stun, reader_and_writer};
use crate::service_request_manager_udp::service_connection_fib_handler;
use fogrs_common::fib_structs::RoutingManagerRequest;
use fogrs_common::fib_structs::{FibChangeAction, FibConnectionType, FibStateChange};
use fogrs_common::packet_structs::{get_gdp_name_from_topic, GDPName, GDPPacket};
use fogrs_ros::TopicManagerRequest;
use futures::StreamExt;
use redis_async::client;
use redis_async::resp::FromResp;
use tokio::net::UdpSocket;
use crate::db::{add_entity_to_database_as_transaction, allow_keyspace_notification};
use crate::db::{get_entity_from_database, get_redis_address_and_port, get_redis_url};
use std::net::{SocketAddr};

use serde::{Deserialize, Serialize};

use core::panic;
use std::env;
use std::str;
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender}; // TODO: replace it out
                                                             // use fogrs_common::fib_structs::TopicManagerAction;
use tokio::sync::mpsc::{self};

fn flip_direction(direction: &str) -> Option<String> {
    let mapping = [
        ("request-receiver", "request-sender"),
        ("response-sender", "response-receiver"),
        ("request-sender", "request-receiver"),
        ("response-receiver", "response-sender"),
        // ("pub-receiver", "sub-sender"),
        // ("sub-sender", "pub-receiver"),
        // ("pub-sender", "sub-receiver"),
        // ("sub-receiver", "pub-sender"),
        ("SENDER-sender", "RECEIVER-receiver"),
        ("RECEIVER-receiver", "SENDER-sender"),
    ];
    info!("direction {:?}", direction);
    for (k, v) in mapping.iter() {
        if k == &direction {
            return Some(v.to_string());
        }
    }
    panic!("Invalid direction {:?}", direction);
}


// protocol: 
// key: <topic_name>-sender, value: [a list of sender gdp names]
// key: <topic_name>-receiver, value: [a list of receiver gdp names]
pub async fn register_stream_sender(
    topic_gdp_name: GDPName,
    direction: String,
    fib_tx: UnboundedSender<GDPPacket>,
    channel_tx: UnboundedSender<FibStateChange>,
) {
    let stream = UdpSocket::bind("0.0.0.0:0").await.unwrap();
    let sock_public_addr = get_socket_stun(&stream).await.unwrap();
    info!("UDP socket is bound to {:?}", sock_public_addr); //TODO: does it matter??

    let direction: &str = direction.as_str();
    let redis_url = get_redis_url();
    let _ = add_entity_to_database_as_transaction(
        &redis_url,
        format!("{:?}-{:}", topic_gdp_name, direction).as_str(),
        sock_public_addr.to_string().as_str(),
    );
    info!(
        "registered {:?} with {:?}",
        topic_gdp_name, sock_public_addr
    );

    let receiver_topic = format!(
        "{:?}-{:}",
        topic_gdp_name,
        flip_direction(direction).unwrap()
    );
    let redis_url = get_redis_url();
    let updated_receivers = get_entity_from_database(&redis_url, &receiver_topic)
        .expect("Cannot get receiver from database");
    info!(
        "get a list of {:?} from KVS {:?}",
        flip_direction(direction),
        updated_receivers
    );

    if updated_receivers.len() != 0 {
        // let receiver_addr = updated_receivers[0].clone();
        // let receiver_socket_addr: SocketAddr = receiver_addr
        //     .parse()
        //     .expect("Failed to parse receiver address");
        for receiver_addr in updated_receivers {
            let stream = UdpSocket::bind("0.0.0.0:0").await.unwrap();
            let receiver_socket_addr: SocketAddr = receiver_addr
                .parse()
                .expect("Failed to parse receiver address");
            let _ = stream.connect(receiver_socket_addr).await;
            info!("connected to {:?}", receiver_socket_addr);
            let (local_to_rtc_tx, local_to_rtc_rx) = mpsc::unbounded_channel();
            let fib_clone = fib_tx.clone();
            tokio::spawn(
                async move {
                    reader_and_writer (
                        stream,
                        fib_clone,
                        // ebpf_tx,
                        local_to_rtc_rx,
                    )
                }
            );
            // send to fib an update 
            let channel_update_msg = FibStateChange {
                action: FibChangeAction::ADD,
                topic_gdp_name: topic_gdp_name,
                connection_type: FibConnectionType::SENDER,
                forward_destination: Some(local_to_rtc_tx),
                description: Some(format!(
                    "udp stream for topic_name {:?}",
                    topic_gdp_name,
                )),
            };
            let _ = channel_tx.send(channel_update_msg);
        }
    }

    // TODO: fix following code later, assume listener start before writer
    let redis_addr_and_port = get_redis_address_and_port();
    let pubsub_con = client::pubsub_connect(redis_addr_and_port.0, redis_addr_and_port.1)
        .await
        .expect("Cannot connect to Redis");
    let redis_topic_stream_name: String = format!("__keyspace@0__:{}", receiver_topic);
    allow_keyspace_notification(&redis_url).expect("Cannot allow keyspace notification");
    let mut msgs = pubsub_con
        .psubscribe(&redis_topic_stream_name)
        .await
        .expect("Cannot subscribe to topic");
    info!("subscribed to {:?}", redis_topic_stream_name);

    loop {
        let message = msgs.next().await;
        match message {
            Some(message) => {
                let received_operation = String::from_resp(message.unwrap()).unwrap();
                info!("KVS {}", received_operation);
                if received_operation != "lpush" {
                    info!("the operation is not lpush, ignore");
                    continue;
                }
                let updated_receivers = get_entity_from_database(&redis_url, &receiver_topic)
                    .expect("Cannot get receiver from database");
                info!("get a list of receivers from KVS {:?}", updated_receivers);
            }
            None => {
                info!("No message received");
            }
        }
    }
}


pub async fn register_stream_receiver(
    topic_gdp_name: GDPName,
    direction: String,
) {
    let stream = UdpSocket::bind("0.0.0.0:0").await.unwrap();
    let sock_public_addr = get_socket_stun(&stream).await.unwrap();
    info!("UDP socket is bound to {:?}", sock_public_addr);

    let direction: &str = direction.as_str();
    let redis_url = get_redis_url();
    let _ = add_entity_to_database_as_transaction(
        &redis_url,
        format!("{:?}-{:}", topic_gdp_name, direction).as_str(),
        sock_public_addr.to_string().as_str(),
    );
    info!(
        "registered {:?} with {:?}",
        topic_gdp_name, sock_public_addr
    );

    let sender_topic = format!(
        "{:?}-{:}",
        topic_gdp_name,
        flip_direction(direction).unwrap()
    );
    let redis_url = get_redis_url();
    let updated_receivers = get_entity_from_database(&redis_url, &sender_topic)
        .expect("Cannot get receiver from database");
    info!(
        "get a list of {:?} from KVS {:?}",
        flip_direction(direction),
        updated_receivers
    );

    if updated_receivers.len() != 0 {
        let receiver_addr = updated_receivers[0].clone();
        let receiver_socket_addr: SocketAddr = receiver_addr
            .parse()
            .expect("Failed to parse receiver address");
    }


    // TODO: fix following code later, assume listener start before writer
    let redis_addr_and_port = get_redis_address_and_port();
    let pubsub_con = client::pubsub_connect(redis_addr_and_port.0, redis_addr_and_port.1)
        .await
        .expect("Cannot connect to Redis");
    let redis_topic_stream_name: String = format!("__keyspace@0__:{}", receiver_topic);
    allow_keyspace_notification(&redis_url).expect("Cannot allow keyspace notification");
    let mut msgs = pubsub_con
        .psubscribe(&redis_topic_stream_name)
        .await
        .expect("Cannot subscribe to topic");
    info!("subscribed to {:?}", redis_topic_stream_name);

    loop {
        let message = msgs.next().await;
        match message {
            Some(message) => {
                let received_operation = String::from_resp(message.unwrap()).unwrap();
                info!("KVS {}", received_operation);
                if received_operation != "lpush" {
                    info!("the operation is not lpush, ignore");
                    continue;
                }
                let updated_receivers = get_entity_from_database(&redis_url, &receiver_topic)
                    .expect("Cannot get receiver from database");
                info!("get a list of receivers from KVS {:?}", updated_receivers);
            }
            None => {
                info!("No message received");
            }
        }
    }
}

#[derive(Clone)]
pub struct RoutingManager {
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
        let connection_type = FibConnectionType::SENDER;
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

                warn!(
                    "sender_network_routing_thread_manager {:?}",
                    connection_type
                );

                let (local_to_rtc_tx, local_to_rtc_rx) = mpsc::unbounded_channel();
                let channel_tx_clone = channel_tx.clone();
                let _rtc_handle = tokio::spawn(
                    async move {
                        let direction = format!("{:?}-{}", connection_type, "sender");

                        register_stream_sender(
                            topic_gdp_name,
                            direction,
                            fib_tx.clone(),
                            channel_tx_clone,
                        );
                    }
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

    pub async fn handle_receiver_routing(
        &self, mut request_rx: UnboundedReceiver<RoutingManagerRequest>,
    ) {
        let connection_type = FibConnectionType::RECEIVER;
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
                let _rtc_handle = tokio::spawn(
                    reader_and_writer(
                    topic_gdp_name,
                    format!("{:?}-{}", connection_type, "receiver"),
                    fib_tx,
                    // ebpf_tx,
                    local_to_rtc_rx,
                ));
            });
        }
    }

    pub async fn handle_client_routing(&self, request_rx: UnboundedReceiver<TopicManagerRequest>) {
        warn!("client routing not implemented yet!");
    }

    pub async fn handle_service_routing(&self, request_rx: UnboundedReceiver<TopicManagerRequest>) {
        warn!("service routing not implemented yet!");
    }
}