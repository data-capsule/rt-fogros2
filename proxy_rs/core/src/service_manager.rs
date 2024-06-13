use crate::api_server::ROSTopicRequest;


use crate::service_request_manager_webrtc::service_connection_fib_handler;
use fogrs_common::fib_structs::RoutingManagerRequest;
use fogrs_common::fib_structs::{FibChangeAction, FibConnectionType, FibStateChange};
use fogrs_common::packet_structs::{get_gdp_name_from_topic, GDPName, GDPPacket};

use serde::{Deserialize, Serialize};

use crate::network::udp::reader_and_writer;
use core::panic;
use std::env;
use std::str;
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender}; // TODO: replace it out
                                                             // use fogrs_common::fib_structs::TopicManagerAction;
use tokio::sync::mpsc::{self};


use fogrs_ros::{ROSManager, TopicManagerAction, TopicManagerRequest};

#[derive(Debug, Clone)]
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

    // pub async fn handle_publisher_routing(
    //     &self, mut request_rx: UnboundedReceiver<TopicManagerRequest>,
    // ) {
    //     let connection_type = FibConnectionType::PUBLISHER;
    //     while let Some(request) = request_rx.recv().await {
    //         let fib_tx = self.fib_tx.clone();
    //         let channel_tx = self.channel_tx.clone();
    //         // let ebpf_tx = self.ebpf_tx.clone();
    //         tokio::spawn(async move {
    //             let topic_name = request.topic_name.clone();
    //             let topic_type = request.topic_type.clone();
    //             let certificate = request.certificate.clone();
    //             let topic_gdp_name = GDPName(get_gdp_name_from_topic(
    //                 &topic_name,
    //                 &topic_type,
    //                 &certificate,
    //             ));

    //             warn!(
    //                 "sender_network_routing_thread_manager {:?}",
    //                 connection_type
    //             );

    //             let (local_to_rtc_tx, local_to_rtc_rx) = mpsc::unbounded_channel();
    //             let _rtc_handle = tokio::spawn(reader_and_writer(
    //                 topic_gdp_name,
    //                 format!("{:?}-{}", connection_type, "sender"),
    //                 fib_tx,
    //                 // ebpf_tx,
    //                 local_to_rtc_rx,
    //             ));
    //             let channel_update_msg = FibStateChange {
    //                 action: FibChangeAction::ADD,
    //                 topic_gdp_name: topic_gdp_name,
    //                 connection_type: connection_type,
    //                 forward_destination: Some(local_to_rtc_tx),
    //                 description: Some(format!(
    //                     "udp stream for topic_name {}, topic_type {}, connection_type {:?}",
    //                     topic_name, topic_type, connection_type
    //                 )),
    //             };
    //             let _ = channel_tx.send(channel_update_msg);
    //             info!("remote sender sent channel update message");
    //         });
    //     }
    // }

    // pub async fn handle_subscriber_routing(
    //     &self, mut request_rx: UnboundedReceiver<TopicManagerRequest>,
    // ) {
    //     let connection_type = FibConnectionType::RECEIVER;
    //     while let Some(request) = request_rx.recv().await {
    //         let fib_tx = self.fib_tx.clone();
    //         // let ebpf_tx = self.ebpf_tx.clone();
    //         tokio::spawn(async move {
    //             let topic_name = request.topic_name.clone();
    //             let topic_type = request.topic_type.clone();
    //             let certificate = request.certificate.clone();
    //             let topic_gdp_name = GDPName(get_gdp_name_from_topic(
    //                 &topic_name,
    //                 &topic_type,
    //                 &certificate,
    //             ));
    //             let (_local_to_rtc_tx, local_to_rtc_rx) = mpsc::unbounded_channel();
    //             let _rtc_handle = tokio::spawn(reader_and_writer(
    //                 topic_gdp_name,
    //                 format!("{:?}-{}", connection_type, "receiver"),
    //                 fib_tx,
    //                 // ebpf_tx,
    //                 local_to_rtc_rx,
    //             ));
    //         });
    //     }
    // }


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
                let _rtc_handle = tokio::spawn(reader_and_writer(
                    topic_gdp_name,
                    format!("{:?}-{}", connection_type, "sender"),
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
                let _rtc_handle = tokio::spawn(reader_and_writer(
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

fn read_certificate() -> Vec<u8> {
    let crypto_name = "test_cert";
    let crypto_path = match env::var_os("SGC_CRYPTO_PATH") {
        Some(config_file) => config_file.into_string().unwrap(),
        None => format!(
            "./sgc_launch/configs/crypto/{}/{}-private.pem",
            crypto_name, crypto_name
        ),
    };

    let certificate = std::fs::read(crypto_path).expect("crypto file not found!");
    return certificate;
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

    let certificate = read_certificate();

    // let _service_provider_handle = tokio::spawn(ros_manager.handle_service_provider(service_request_rx, fib_tx.clone(), channel_tx.clone()));

    // let _service_provider_handle = tokio::spawn(ROSManager::handle_service_provider(service_request_rx, fib_tx.clone(), channel_tx.clone()));

    let _fib_handle = tokio::spawn(async move {
        service_connection_fib_handler(fib_rx, channel_rx).await;
    });

    // Similarly, spawn other necessary tasks for `ros_local_service_caller`, `ros_topic_remote_publisher`, etc.

    // let (_sender_routing_handle, sender_routing_rx) = mpsc::unbounded_channel();
    // tokio::spawn(routing_manager.handle_sender_routing(sender_routing_rx));
    let routing_manager_clone = routing_manager.clone();
    let (client_operation_tx, client_operation_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        routing_manager_clone
            .handle_client_routing(client_operation_rx)
            .await;
    });

    let (service_operation_tx, service_operation_rx) = mpsc::unbounded_channel();
    let routing_manager_clone = routing_manager.clone();
    tokio::spawn(async move {
        routing_manager_clone
            .handle_service_routing(service_operation_rx)
            .await;
    });

    // TODO: this will be created under ROS
    let (publisher_operation_tx, publisher_operation_rx) = mpsc::unbounded_channel();
    let ros_manager_clone = ros_manager.clone();
    let fib_tx_clone = fib_tx.clone();
    let channel_tx_clone = channel_tx.clone();
    tokio::spawn(async move {
        ros_manager_clone.handle_ros_topic_remote_publisher(
            publisher_operation_rx,
            fib_tx_clone,
            channel_tx_clone,
        ).await;
    });

    let (subscriber_operation_tx, subscriber_operation_rx) = mpsc::unbounded_channel();
    let ros_manager_clone = ros_manager.clone();
    let fib_tx_clone = fib_tx.clone();
    let channel_tx_clone = channel_tx.clone();
    tokio::spawn(async move {
        ros_manager_clone.handle_ros_topic_remote_subscriber(
            subscriber_operation_rx,
            fib_tx_clone,
            channel_tx_clone,
        ).await;
    });

    let (sender_routing_tx, sender_routing_rx) = mpsc::unbounded_channel();
    let routing_manager_clone = routing_manager.clone();
    tokio::spawn(async move {
        routing_manager_clone
            .handle_sender_routing(sender_routing_rx)
            .await;
    });

    let (receiver_routing_tx, receiver_routing_rx) = mpsc::unbounded_channel();
    let routing_manager_clone = routing_manager.clone();
    tokio::spawn(async move {
        routing_manager_clone
            .handle_receiver_routing(receiver_routing_rx)
            .await;
    });


    // let (_receiver_routing_handle, receiver_routing_rx) = mpsc::unbounded_channel();
    // tokio::spawn(routing_manager.handle_receiver_routing(receiver_routing_rx));

    // Continuously monitor service requests and handle them accordingly.

    loop {
        select! {
            Some(payload) = service_request_rx.recv() => {
                // info!("ros topic manager get payload: {:?}", payload);
                match payload.api_op.as_str() {
                    "add" => {
                        let topic_name = payload.topic_name;
                        let topic_type = payload.topic_type;
                        let action = payload.ros_op;
                        let topic_gdp_name = GDPName(get_gdp_name_from_topic(
                            &topic_name,
                            &topic_type,
                            &certificate,
                        ));
                        info!("detected a new topic {:?} with action {:?}, topic gdpname {:?}", topic_name, action, topic_gdp_name);
                        match action.as_str() {
                            // provide service locally and send to remote service
                            "client" => {
                                let topic_name_clone = topic_name.clone();
                                let certificate = certificate.clone();
                                let topic_creator_request = TopicManagerRequest {
                                    action: TopicManagerAction::ADD,
                                    topic_name: topic_name_clone,
                                    topic_type: topic_type,
                                    certificate: certificate,
                                };
                                let _ = client_operation_tx.send(topic_creator_request);
                            }

                            // provide service remotely and interact with local service, and send back
                            "service" => {
                                let topic_name_clone = topic_name.clone();
                                let certificate = certificate.clone();
                                let topic_type = topic_type.clone();
                                let topic_creator_request = TopicManagerRequest {
                                    action: TopicManagerAction::ADD,
                                    topic_name: topic_name_clone,
                                    topic_type: topic_type,
                                    certificate: certificate,
                                };
                                let _ = service_operation_tx.send(topic_creator_request);
                            },
                            "pub" => {
                                let topic_name_clone = topic_name.clone();
                                let certificate = certificate.clone();
                                let topic_creator_request = TopicManagerRequest {
                                    action: TopicManagerAction::ADD,
                                    topic_name: topic_name_clone,
                                    topic_type: topic_type,
                                    certificate: certificate,
                                };
                                let _ = publisher_operation_tx.send(topic_creator_request);
                            }

                            // provide service remotely and interact with local service, and send back
                            "sub" => {
                                let topic_name_clone = topic_name.clone();
                                let certificate = certificate.clone();
                                let topic_type = topic_type.clone();
                                let topic_creator_request = TopicManagerRequest {
                                    action: TopicManagerAction::ADD,
                                    topic_name: topic_name_clone,
                                    topic_type: topic_type,
                                    certificate: certificate,
                                };
                                let _ = subscriber_operation_tx.send(topic_creator_request);
                            },
                            _ => {
                                warn!("unknown action {}", action);
                            }
                        };
                    },
                    "routing" => {
                        info!("routing operation {:?}", payload);

                        let topic_name = payload.topic_name;
                        let topic_type = payload.topic_type;
                        let action = payload.ros_op;

                        match action.as_str() {

                            // source: fib -> webrtc
                            "source" => {
                                sender_routing_tx.send(RoutingManagerRequest {
                                    action: FibChangeAction::ADD,
                                    topic_name: topic_name,
                                    topic_type: topic_type,
                                    certificate: certificate.clone(),
                                    connection_type: Some(payload.connection_type.unwrap()),
                                    communication_url: Some(payload.forward_sender_url.unwrap()),
                                }).expect("sender routing tx failure");
                            }

                            // destination: webrtc -> fib
                            "destination" => {
                                receiver_routing_tx.send(RoutingManagerRequest {
                                    action: FibChangeAction::ADD,
                                    topic_name: topic_name,
                                    topic_type: topic_type,
                                    certificate: certificate.clone(),
                                    connection_type: Some(payload.connection_type.unwrap()),
                                    communication_url: Some(payload.forward_receiver_url.unwrap()),
                                }).expect("receiver routing tx failure");
                            }

                            "pub" => {
                                sender_routing_tx.send(RoutingManagerRequest {
                                    action: FibChangeAction::ADD,
                                    topic_name: topic_name,
                                    topic_type: topic_type,
                                    certificate: certificate.clone(),
                                    connection_type: Some(payload.connection_type.unwrap()),
                                    communication_url: Some(payload.forward_sender_url.unwrap()),
                                }).expect("sender routing tx failure");
                            }

                            // destination: webrtc -> fib
                            "sub" => {
                                receiver_routing_tx.send(RoutingManagerRequest {
                                    action: FibChangeAction::ADD,
                                    topic_name: topic_name,
                                    topic_type: topic_type,
                                    certificate: certificate.clone(),
                                    connection_type: Some(payload.connection_type.unwrap()),
                                    communication_url: Some(payload.forward_receiver_url.unwrap()),
                                }).expect("receiver routing tx failure");
                            }

                            _ => {
                                warn!("unknown action {}", action);
                            }
                        }
                    }

                    _ => {
                        info!("operation {} not handled!", payload.api_op);
                    }
                }
            },
        }
    }
}
