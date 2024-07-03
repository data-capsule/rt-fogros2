use crate::db::{add_entity_to_database_as_transaction, allow_keyspace_notification};
use crate::db::{get_entity_from_database, get_redis_address_and_port, get_redis_url};
use crate::network::udp::get_socket_stun;
use default_net::interface::get_default_interface_name;
use fogrs_common::fib_structs::RoutingManagerRequest;
use fogrs_common::fib_structs::{FibChangeAction, FibConnectionType, FibStateChange};
use fogrs_common::packet_structs::{
    generate_random_gdp_name, get_gdp_name_from_topic, GDPName, GDPPacket,
};
use fogrs_kcp::to_kcp_config;
use fogrs_kcp::KcpListener;
use fogrs_ros::TopicManagerRequest;
use futures::StreamExt;
use redis_async::client;
use redis_async::resp::FromResp;
use std::net::SocketAddr;
use tokio::net::UdpSocket;

use core::panic;
use std::str;
use std::vec;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender}; // TODO: replace it out
                                                             // use fogrs_common::fib_structs::TopicManagerAction;
use tokio::sync::mpsc::{self};

use libc::{c_int, c_void, setsockopt, SOL_SOCKET, SO_BINDTODEVICE};
use pnet::datalink::{self};
use std::ffi::CString;
use std::os::unix::io::AsRawFd;

const transmission_protocol: &str = "kcp";


fn direction_str_to_connection_type(connection_type: &str) -> FibConnectionType {
    match connection_type.to_uppercase().as_str() {
        "PUBSUB-SENDER" => FibConnectionType::SENDER,
        "PUBSUB-RECEIVER" => FibConnectionType::RECEIVER,
        "REQUEST-SENDER" => FibConnectionType::REQUESTSENDER,
        "REQUEST-RECEIVER" => FibConnectionType::REQUESTRECEIVER,
        "RESPONSE-SENDER" => FibConnectionType::RESPONSESENDER,
        "RESPONSE-RECEIVER" => FibConnectionType::RESPONSERECEIVER,
        _ => panic!("Invalid connection type {:?}", connection_type),
    }
}


fn flip_direction(direction: &str) -> Option<String> {
    let mapping = [
        ("request-receiver", "request-sender"),
        ("request-sender", "request-receiver"),
        ("response-sender", "response-receiver"),
        ("response-receiver", "response-sender"),
        ("pubsub-sender", "pubsub-receiver"),
        ("pubsub-receiver", "pubsub-sender"),
    ];
    info!("direction {:?}", direction);
    for (k, v) in mapping.iter() {
        if k == &direction.to_lowercase() {
            return Some(v.to_string());
        }
    }
    panic!("Invalid direction {:?}", direction);
}


fn get_ip_address(interface_name: &str) -> Option<SocketAddr> {
    let interfaces = datalink::interfaces();
    for interface in interfaces {
        if interface.name == interface_name {
            for ip in interface.ips {
                if let std::net::IpAddr::V4(ipv4) = ip.ip() {
                    return Some(SocketAddr::new(ip.ip(), 0));
                }
            }
        }
    }
    None
}


fn bind_to_interface(socket: &UdpSocket, interface: &str) -> std::io::Result<()> {
    let cstr = CString::new(interface).unwrap();
    let fd = socket.as_raw_fd();
    let ret = unsafe {
        setsockopt(
            fd,
            SOL_SOCKET,
            SO_BINDTODEVICE,
            cstr.as_ptr() as *const c_void,
            (cstr.to_bytes_with_nul().len() as c_int)
                .try_into()
                .unwrap(),
        )
    };

    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}


// protocol:
// requirement, receiver connection needs to be created before sender
// key: {<topic_name>-sender}, value: [a list of sender gdp names]
// key: {<topic_name>-receiver}, value: [a list of [sender-receiver] gdp names]
// key: {[sender-receiver]}, value: IP address of receiver
// receiver: watch for {<topic_name>-sender}, if there is a new sender,
//          1. put value {IP_address} to key {[sender-receiver]}
//          2. append value [sender_gdp_name, receiver_gdp_name] to {<topic_name>-receiver}
// sender : watch for [sender_gdp_name, receiver_gdp_name] in {<topic_name>-receiver}, if sender_gdp_name is in the list, query the value and connect

pub async fn register_stream_sender(
    topic_gdp_name: GDPName, direction: String, fib_tx: UnboundedSender<GDPPacket>,
    channel_tx: UnboundedSender<FibStateChange>, interface: &str, config: fogrs_kcp::KcpConfig,
) {
    let direction: &str = direction.as_str();
    let redis_url = get_redis_url();
    let sender_key_name = format!("{}-{:}", topic_gdp_name, &direction);
    let receiver_key_name = format!(
        "{}-{:}",
        topic_gdp_name,
        flip_direction(&direction).unwrap()
    );
    let sender_thread_gdp_name = generate_random_gdp_name();

    let redis_addr_and_port = get_redis_address_and_port();
    let pubsub_con = client::pubsub_connect(redis_addr_and_port.0, redis_addr_and_port.1)
        .await
        .expect("Cannot connect to Redis");
    let redis_topic_stream_name: String = format!("__keyspace@0__:{}", receiver_key_name);
    allow_keyspace_notification(&redis_url).expect("Cannot allow keyspace notification");
    let mut msgs = pubsub_con
        .psubscribe(&redis_topic_stream_name)
        .await
        .expect("Cannot subscribe to topic");
    info!("subscribed to {:?}", redis_topic_stream_name);

    let sender_thread_gdp_name_str = sender_thread_gdp_name.to_string();
    let _ = add_entity_to_database_as_transaction(
        &redis_url,
        format!("{}-{:}", topic_gdp_name, direction).as_str(),
        sender_thread_gdp_name_str.as_str(),
    );

    info!(
        "registered {:?} with {:?}",
        topic_gdp_name, sender_thread_gdp_name
    );

    let mut processed_receivers = vec![];

    loop {
        tokio::select! {
        Some(message) = msgs.next() => {
                info!("msg {:?}", message);
                let received_operation = String::from_resp(message.unwrap()).unwrap();

                if received_operation != "lpush" {
                    info!("the operation is not lpush, ignore");
                    continue;
                }
                let updated_receivers = get_entity_from_database(&redis_url, &receiver_key_name)
                    .expect("Cannot get receiver from database");
                info!("get a list of receivers from KVS {:?}", updated_receivers);
                let new_receivers = updated_receivers
                    .iter()
                    .filter(|&r| !processed_receivers.contains(r))
                    .collect::<Vec<_>>();

                for receiver_channel in new_receivers { //format: sender_thread_gdp_name_str-receiver_thread_gdp_name_str
                    info!("new receiver {:?}", receiver_channel);
                    processed_receivers.push(receiver_channel.to_string());
                    // check if value starts with sender_thread_gdp_name_str
                    if !receiver_channel.starts_with(&sender_thread_gdp_name_str) {
                        info!(
                            "receiver_channel {:?}, not starting with {}",
                            receiver_channel, sender_thread_gdp_name_str
                        );
                        continue;
                    }

                    // query value of key [sender_gdp_name, receiver_gdp_name] to be the receiver_addr
                    let receiver_addr = &get_entity_from_database(&redis_url, &receiver_channel)
                        .expect("Cannot get receiver from database")[0];
                    info!("receiver_addr {:?}", receiver_addr);

                    let receiver_socket_addr: SocketAddr = receiver_addr
                        .parse()
                        .expect("Failed to parse receiver address");

                    let stream = UdpSocket::bind("0.0.0.0:0").await.unwrap();

                    bind_to_interface(&stream, interface).expect("Cannot bind to interface");

                    if transmission_protocol == "kcp" {
                        let _ = fogrs_kcp::KcpStream::connect(&config, receiver_socket_addr).await.unwrap();
                        info!("connected to {:?}", receiver_socket_addr);
                    } else if transmission_protocol == "udp" {
                        let _ = stream.connect(receiver_socket_addr).await;
                    }

                    let (local_to_net_tx, local_to_net_rx) = mpsc::unbounded_channel();
                    let fib_clone = fib_tx.clone();
                    tokio::spawn(async move {
                        if transmission_protocol == "kcp" {
                            let stream = fogrs_kcp::KcpStream::connect(&config, receiver_socket_addr).await.unwrap();
                            info!("connected to {:?}", receiver_socket_addr);
                            crate::network::kcp::reader_and_writer(
                                stream,
                                fib_clone,
                                // ebpf_tx,
                                local_to_net_rx,
                            ).await;
                        } else if transmission_protocol == "udp" {
                            let _ = stream.connect(receiver_socket_addr).await;
                            crate::network::udp::reader_and_writer(
                                stream,
                                fib_clone,
                                // ebpf_tx,
                                local_to_net_rx,
                            ).await;
                        }
                    });
                    // send to fib an update
                    let channel_update_msg = FibStateChange {
                        action: FibChangeAction::ADD,
                        topic_gdp_name: topic_gdp_name,
                        // here is a little bit tricky:
                        //  to fib, it is the receiver
                        // it connects to a remote receiver
                        connection_type: direction_str_to_connection_type(flip_direction(direction).unwrap().as_str()),
                        forward_destination: Some(local_to_net_tx),
                        description: Some(format!(
                            "udp stream sending for topic_name {:?} to address {:?} direction {:?}",
                            topic_gdp_name, receiver_addr, direction
                        )),
                    };
                    let _ = channel_tx.send(channel_update_msg);
                }

            }
            // _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {
            //     info!("waiting for redis message");
            // }
        }
    } // loop
}


// protocol:
// requirement, receiver connection needs to be created before sender
// key: {<topic_name>-sender}, value: [a list of sender gdp names]
// key: {<topic_name>-receiver}, value: [a list of [sender-receiver] gdp names]
// key: {[sender-receiver]}, value: IP address of receiver
// receiver: watch for {<topic_name>-sender}, if there is a new sender,
//          1. put value {IP_address} to key {[sender-receiver]}
//          2. append value [sender_gdp_name, receiver_gdp_name] to {<topic_name>-receiver}
// sender : watch for [sender_gdp_name, receiver_gdp_name] in {<topic_name>-receiver}, if sender_gdp_name is in the list, query the value and connect

pub async fn receiver_registration_handler(
    topic_gdp_name: GDPName, receiver_key_name: String, sender_key_name: String, direction: &str,
    fib_tx: UnboundedSender<GDPPacket>, channel_tx: UnboundedSender<FibStateChange>,
    processed_senders: &mut Vec<String>, interface: &str, config: fogrs_kcp::KcpConfig,
) {
    let redis_url = get_redis_url();

    let updated_senders = get_entity_from_database(&redis_url, &sender_key_name)
        .expect("Cannot get sender from database");
    info!("get a list of senders from KVS {:?}", updated_senders);

    for sender_gdp_name in updated_senders {
        info!("new sender {:?}", sender_gdp_name);
        if processed_senders.contains(&sender_gdp_name) {
            info!("the sender is already processed, ignore");
            continue;
        }
        processed_senders.push(sender_gdp_name.to_string());
        // put value {IP_address} to key {[sender-receiver]}

        let stream = UdpSocket::bind("0.0.0.0:0").await.unwrap();

        bind_to_interface(&stream, interface).expect("Cannot bind to interface");

        let sock_public_addr = match get_socket_stun(&stream).await {
            Ok(addr) => addr,
            Err(err) => {
                let mut pnet_addr = get_ip_address(interface).unwrap();
                pnet_addr.set_port(stream.local_addr().unwrap().port());
                warn!(
                    "Failed to get public address from stun, error: {}, using pnet addr {}",
                    err, pnet_addr
                );
                pnet_addr
            }
        };


        info!("UDP socket is bound to {:?}", sock_public_addr);

        let fib_tx_clone = fib_tx.clone();
        let channel_tx_clone = channel_tx.clone();
        let receiver_key_name = receiver_key_name.clone();
        let redis_url = redis_url.clone();
        let direction = direction.to_string();
        tokio::spawn(async move {
            // reader_and_writer(
            //     stream,
            //     fib_tx_clone,
            //     // ebpf_tx,
            //     local_to_net_rx,
            // )
            // .await;

            let sender_receiver_key = format!("{}-{:}", sender_gdp_name, receiver_key_name);
            let _ = add_entity_to_database_as_transaction(
                &redis_url,
                &sender_receiver_key,
                format!("{}", sock_public_addr).as_str(),
            );
            info!(
                "registered {:?} with {:?}",
                sender_receiver_key, sock_public_addr
            );

            // append value [sender_gdp_name, receiver_gdp_name] to {<topic_name>-receiver}
            let sender_receiver_value = format!("{}-{:}", sender_gdp_name, receiver_key_name);
            let _ = add_entity_to_database_as_transaction(
                &redis_url,
                &receiver_key_name,
                sender_receiver_value.as_str(),
            );
            info!(
                "registered {:?} with {:?}",
                sender_gdp_name, receiver_key_name
            );


            let (local_to_net_tx, local_to_net_rx) = mpsc::unbounded_channel();
            tokio::spawn(async move {
                if transmission_protocol == "kcp" {
                    let mut listener = KcpListener::from_socket(config, stream).await.unwrap();
                    info!("KCP listener is bound to {:?}", sock_public_addr);
                    let (stream, peer_addr) = match listener.accept().await {
                        Ok(s) => s,
                        Err(err) => {
                            error!("accept failed, error: {}", err);
                            return;
                        }
                    };
                    info!("accepted {}", peer_addr);
                    crate::network::kcp::reader_and_writer(stream, fib_tx_clone, local_to_net_rx)
                        .await;
                } else {
                    crate::network::udp::reader_and_writer(stream, fib_tx_clone, local_to_net_rx)
                        .await;
                }
            });
            let channel_update_msg = FibStateChange {
                action: FibChangeAction::ADD,
                topic_gdp_name: topic_gdp_name,
                // connection_type: direction_str_to_connection_type(direction.as_str()), // it connects from a remote sender
                // here is a little bit tricky: 
                //  to fib, it is the receiver
                // it connects to a remote receiver
                connection_type: direction_str_to_connection_type(flip_direction(direction.as_str()).unwrap().as_str()),
                forward_destination: Some(local_to_net_tx),
                description: Some(format!(
                    "udp stream connecting to remote sender for topic_name {:?} bind to address {:?} direction {:?}",
                    topic_gdp_name, sock_public_addr, direction,
                )),
            };
            channel_tx_clone
                .send(channel_update_msg)
                .expect("Cannot send channel update message");
        });
    }
}

pub async fn register_stream_receiver(
    topic_gdp_name: GDPName, direction: String, fib_tx: UnboundedSender<GDPPacket>,
    channel_tx: UnboundedSender<FibStateChange>, interface: &str, config: fogrs_kcp::KcpConfig,
) {
    let direction: &str = direction.as_str();
    let redis_url = get_redis_url();
    let receiver_key_name = format!("{}-{:}", topic_gdp_name, &direction);
    let sender_key_name = format!(
        "{}-{:}",
        topic_gdp_name,
        flip_direction(&direction).unwrap()
    );
    let sender_thread_gdp_name = generate_random_gdp_name();

    let redis_addr_and_port = get_redis_address_and_port();
    let pubsub_con = client::pubsub_connect(redis_addr_and_port.0, redis_addr_and_port.1)
        .await
        .expect("Cannot connect to Redis");
    let redis_topic_stream_name: String = format!("__keyspace@0__:{:}", sender_key_name);
    // let redis_topic_stream_name: String = format!("__keyspace@0__:*");
    allow_keyspace_notification(&redis_url).expect("Cannot allow keyspace notification");
    let mut msgs = pubsub_con
        .psubscribe(&redis_topic_stream_name)
        .await
        .expect("Cannot subscribe to topic");
    info!("subscribed to {:?}", redis_topic_stream_name);

    info!(
        "Attempting to subscribe to topic: {}",
        redis_topic_stream_name
    );

    let current_value_under_key = get_entity_from_database(&redis_url, &sender_key_name)
        .expect("Cannot get sender from database");
    info!(
        "get a list of senders from KVS {:?}",
        current_value_under_key
    );

    let mut processed_senders = vec![];

    // check senders that are already in the KVS
    receiver_registration_handler(
        topic_gdp_name.clone(),
        receiver_key_name.clone(),
        sender_key_name.clone(),
        direction,
        fib_tx.clone(),
        channel_tx.clone(),
        &mut processed_senders,
        interface,
        config,
    )
    .await;

    loop {
        tokio::select! {
            Some(message) = msgs.next() => {
                let received_operation = String::from_resp(message.unwrap()).unwrap();
                info!("KVS {}", received_operation);
                if received_operation != "lpush" {
                    info!("the operation is not lpush, ignore");
                    continue;
                }
                receiver_registration_handler(
                    topic_gdp_name.clone(),
                    receiver_key_name.clone(),
                    sender_key_name.clone(),
                    direction,
                    fib_tx.clone(),
                    channel_tx.clone(),
                    &mut processed_senders,
                    interface,
                    config,
                ).await;
            }

            // check if there is any new sender
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
                receiver_registration_handler(
                    topic_gdp_name.clone(),
                    receiver_key_name.clone(),
                    sender_key_name.clone(),
                    direction,
                    fib_tx.clone(),
                    channel_tx.clone(),
                    &mut processed_senders,
                    interface,
                    config,
                ).await;
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
        loop {
            tokio::select! {
                Some(request) = request_rx.recv() => {
                    let fib_tx = self.fib_tx.clone();
                    let channel_tx = self.channel_tx.clone();
                    // let ebpf_tx = self.ebpf_tx.clone();
                    let topic_name = request.topic_name.clone();
                    let topic_type = request.topic_type.clone();
                    let certificate = request.certificate.clone();
                    let topic_qos = request.topic_qos.clone();
                    let config = to_kcp_config(topic_qos.as_str());
                    let interface = get_default_interface_name().unwrap();
                    let direction = format!("{}-{}", request.connection_type.unwrap(), "sender");
                    let connection_type = direction_str_to_connection_type(
                        direction.as_str()
                    );


                    let topic_gdp_name = GDPName(get_gdp_name_from_topic(
                        &topic_name,
                        &topic_type,
                        &certificate,
                    ));

                    warn!(
                        "sender_network_routing_thread_manager {:?}",
                        connection_type
                    );

                    let (local_to_net_tx, local_to_net_rx) = mpsc::unbounded_channel();
                    let channel_tx_clone = channel_tx.clone();
                    tokio::spawn(async move {

                        register_stream_sender(
                            topic_gdp_name,
                            direction,
                            fib_tx.clone(),
                            channel_tx_clone,
                            interface.as_str(),
                            config,
                        )
                        .await;
                    });

                    let channel_update_msg = FibStateChange {
                        action: FibChangeAction::ADD,
                        topic_gdp_name: topic_gdp_name,
                        connection_type: connection_type,
                        forward_destination: Some(local_to_net_tx),
                        description: Some(format!(
                            "udp stream for topic_name {}, topic_type {}, connection_type {:?}",
                            topic_name, topic_type, connection_type
                        )),
                    };
                    let _ = channel_tx.send(channel_update_msg);
                    info!("remote sender sent channel update message");
                }
                // _ = tokio::time::sleep(tokio::time::Duration::from_secs(1)) => {
                //     info!("waiting for request message");
                // }
            };
        }
    }

    pub async fn handle_receiver_routing(
        &self, mut request_rx: UnboundedReceiver<RoutingManagerRequest>,
    ) {
        let mut handles = vec![];

        loop {
            tokio::select! {
                Some(request) = request_rx.recv() => {
                    let fib_tx = self.fib_tx.clone();
                    let channel_tx = self.channel_tx.clone();
                    // let ebpf_tx = self.ebpf_tx.clone();
                    handles.push(tokio::spawn(async move {
                        let topic_name = request.topic_name.clone();
                        let topic_type = request.topic_type.clone();
                        let certificate = request.certificate.clone();
                        let topic_qos = request.topic_qos.clone();
                        let topic_gdp_name = GDPName(get_gdp_name_from_topic(
                            &topic_name,
                            &topic_type,
                            &certificate,
                        ));
                        let direction = format!("{}-{}", request.connection_type.unwrap(), "receiver");
                        let connection_type = direction_str_to_connection_type(
                            direction.as_str()
                        );

                        let config = to_kcp_config(topic_qos.as_str());
                        let interface = get_default_interface_name().unwrap();

                        warn!(
                            "receiver_network_routing_thread_manager {:?}",
                            connection_type
                        );

                        let (local_to_net_tx, local_to_net_rx) = mpsc::unbounded_channel();
                        let channel_tx_clone = channel_tx.clone();

                        register_stream_receiver(
                            topic_gdp_name,
                            direction,
                            fib_tx.clone(),
                            channel_tx_clone,
                            interface.as_str(),
                            config,
                        )
                        .await;
                        warn!("receiver_network_routing_thread_manager {:?} finished", connection_type);

                        let channel_update_msg = FibStateChange {
                            action: FibChangeAction::ADD,
                            topic_gdp_name: topic_gdp_name,
                            connection_type: connection_type,
                            forward_destination: Some(local_to_net_tx),
                            description: Some(format!(
                                "udp stream for topic_name {}, topic_type {}, connection_type {:?}",
                                topic_name, topic_type, connection_type
                            )),
                        };
                        let _ = channel_tx.send(channel_update_msg);
                        info!("remote sender sent channel update message");

                    }));
                }

            }
        }
    }

    pub async fn handle_client_routing(&self, request_rx: UnboundedReceiver<TopicManagerRequest>) {
        warn!("client routing not implemented yet!");
    }

    pub async fn handle_service_routing(&self, request_rx: UnboundedReceiver<TopicManagerRequest>) {
        warn!("service routing not implemented yet!");
    }
}
