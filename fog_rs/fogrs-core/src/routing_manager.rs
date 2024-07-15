use crate::db::{add_entity_to_database_as_transaction, allow_keyspace_notification};
use crate::db::{get_entity_from_database, get_redis_address_and_port, get_redis_url};
use crate::network::udp::get_socket_stun;
use default_net::interface::get_default_interface_name;
use fogrs_common::fib_structs::RoutingManagerRequest;
use fogrs_common::fib_structs::{FibChangeAction, FibConnectionType, FibStateChange};
use fogrs_common::packet_structs::{
    generate_random_gdp_name, get_gdp_name_from_topic, GDPName, GDPPacket,
};
use fogrs_kcp::{to_kcp_config, KcpStream};
use fogrs_kcp::KcpListener;
use fogrs_ros::TopicManagerRequest;
use futures::StreamExt;
use librice::candidate;
use redis_async::client;
use redis_async::resp::FromResp;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time;
use std::fmt::format;
use std::net::SocketAddr;
use std::time::Duration;
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
use socket2::{Domain, Protocol, SockAddr, Socket, Type};

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

fn gather_candidate_interfaces() -> Vec<String> {
    let interfaces = datalink::interfaces();
    let mut candidate_interfaces = vec![];
    for interface in interfaces {
         // return interface name
         candidate_interfaces.push(interface.name);
    }
    candidate_interfaces
}

async fn send_ping_from_interface(interface_name: &str, remote_ip_addr:SocketAddr) -> std::io::Result<()> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).unwrap();

    let socket = UdpSocket::from_std(socket.into()).unwrap();
    // TODO: bind to interface
    bind_to_interface(&socket, interface_name).unwrap();

    let buffer = b"ping";
    let mut stream = match KcpStream::connect_with_socket(&to_kcp_config("fast"),socket, remote_ip_addr).await
    {
        Ok(s) => s,
        Err(err) => {
            error!("connect failed, error: {}", err);
            return Err(err.into());
        }
    };

    match stream.write_all(&buffer[..4]).await
    {
        Ok(_) => (),
        Err(err) => {
            error!("write failed, error: {}", err);
            return Err(err);
        }
    };
    stream.write_all(&buffer[..4]).await.unwrap();

    stream.flush().await.unwrap();

    info!("ping sent from interface {} to {}", interface_name, remote_ip_addr);
    let mut buf = [0; 1024];
    loop{
        tokio::select! {
            Ok(len) = stream.read(&mut buf) => {
                let response = str::from_utf8(&buf[..len]).unwrap();
                info!("ping response from interface {} is {}", interface_name, response);
                if response != "pong" {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "ping response is not pong",
                    ));
                }else{
                    break;
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(1000)) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "ping response is not received",
                ));
            }
        }
    }
    Ok(())
}

async fn get_latency_for_remote_ip_addr_from_all_interfaces(
    remote_ip_addr: SocketAddr,
) -> Vec<(String, std::time::Duration)> {
    let interfaces = datalink::interfaces();
    let mut latencies = vec![];
    for interface in interfaces {
        let interface_name = interface.name;
        //start a timer
        let start_time = std::time::Instant::now();

        let latency = match send_ping_from_interface(&interface_name, remote_ip_addr).await {
            Ok(_) => {
                info!("ping from interface {} to remote ip address {} is successful", interface_name, remote_ip_addr);
                start_time.elapsed()
            }
            Err(e) => {
                warn!("ping from interface {} to remote ip address {} is failed, error: {}", interface_name, remote_ip_addr, e);
                std::time::Duration::from_secs(1000)
            }
        };
        latencies.push((interface_name, latency));
    }
    latencies
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

pub async fn opening_side_socket_handler(
    topic_gdp_name: GDPName, direction: String,
    fib_tx: UnboundedSender<GDPPacket>, channel_tx: UnboundedSender<FibStateChange>,
    udp_socket: UdpSocket, sock_public_addr:SocketAddr, config: fogrs_kcp::KcpConfig,
) {
    
    let fib_tx_clone = fib_tx.clone();
    let channel_tx_clone = channel_tx.clone();
    let mut listener = KcpListener::from_socket(config, udp_socket).await.unwrap();
    info!("KCP listener is bound to {:?}", sock_public_addr);
    tokio::spawn(async move {
    loop {
        let (mut stream, peer_addr) = match listener.accept().await {
            Ok(s) => s,
            Err(err) => {
                error!("accept failed, error: {}", err);
                time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };

        info!("accepted {}", peer_addr);
        let fib_tx_clone = fib_tx_clone.clone();
        let (local_to_net_tx, local_to_net_rx) = mpsc::unbounded_channel();
        let direction = direction.clone();
        let channel_tx_clone = channel_tx_clone.clone();
        
        tokio::spawn(async move {
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
                    "udp stream connecting to remote sender for topic_name {:?} bind to address {:?} from {:?} direction {:?}",
                    topic_gdp_name, sock_public_addr, peer_addr, direction,
                )),
            };
            channel_tx_clone
                .send(channel_update_msg)
                .expect("Cannot send channel update message");
            crate::network::kcp::reader_and_writer(stream, fib_tx_clone, local_to_net_rx)
                .await;
            }
        );
        }
    });
}

async fn sender_socket_handler(
    topic_gdp_name: GDPName, receiver_key_name: String, sender_key_name: String, direction: String,
    fib_tx: UnboundedSender<GDPPacket>, channel_tx: UnboundedSender<FibStateChange>,
    stream: UdpSocket, sock_public_addr:SocketAddr, config: fogrs_kcp::KcpConfig,
)
{

    // loop {
    //     tokio::select! {
    //     Some(message) = msgs.next() => {
    //             info!("msg {:?}", message);
    //             let received_operation = String::from_resp(message.unwrap()).unwrap();

    //             if received_operation != "lpush" {
    //                 info!("the operation is not lpush, ignore");
    //                 continue;
    //             }
    //             let updated_receivers = get_entity_from_database(&redis_url, &receiver_key_name)
    //                 .expect("Cannot get receiver from database");
    //             info!("get a list of receivers from KVS {:?}", updated_receivers);
    //             let new_receivers = updated_receivers
    //                 .iter()
    //                 .filter(|&r| !processed_receivers.contains(r))
    //                 .collect::<Vec<_>>();

    //             for receiver_channel in new_receivers { //format: sender_thread_gdp_name_str-receiver_thread_gdp_name_str
    //                 info!("new receiver {:?}", receiver_channel);
    //                 processed_receivers.push(receiver_channel.to_string());
    //                 // check if value starts with sender_thread_gdp_name_str
    //                 if !receiver_channel.starts_with(&sender_thread_gdp_name_str) {
    //                     info!(
    //                         "receiver_channel {:?}, not starting with {}",
    //                         receiver_channel, sender_thread_gdp_name_str
    //                     );
    //                     continue;
    //                 }

    //                 // query value of key [sender_gdp_name, receiver_gdp_name] to be the receiver_addr
    //                 let receiver_addr = &get_entity_from_database(&redis_url, &receiver_channel)
    //                     .expect("Cannot get receiver from database")[0];
    //                 info!("receiver_addr {:?}", receiver_addr);

    //                 let receiver_socket_addr: SocketAddr = receiver_addr
    //                     .parse()
    //                     .expect("Failed to parse receiver address");

    //                 let stream = UdpSocket::bind("0.0.0.0:0").await.unwrap();

    //                 bind_to_interface(&stream, interface).expect("Cannot bind to interface");

    //                 if transmission_protocol == "kcp" {
    //                     let _ = fogrs_kcp::KcpStream::connect(&config, receiver_socket_addr).await.unwrap();
    //                     info!("connected to {:?}", receiver_socket_addr);
    //                 } else if transmission_protocol == "udp" {
    //                     let _ = stream.connect(receiver_socket_addr).await;
    //                 }

    //                 let (local_to_net_tx, local_to_net_rx) = mpsc::unbounded_channel();
    //                 let fib_clone = fib_tx.clone();
    //                 tokio::spawn(async move {
    //                     if transmission_protocol == "kcp" {
    //                         let stream = fogrs_kcp::KcpStream::connect(&config, receiver_socket_addr).await.unwrap();
    //                         info!("connected to {:?}", receiver_socket_addr);
    //                         crate::network::kcp::reader_and_writer(
    //                             stream,
    //                             fib_clone,
    //                             // ebpf_tx,
    //                             local_to_net_rx,
    //                         ).await;
    //                     } else if transmission_protocol == "udp" {
    //                         let _ = stream.connect(receiver_socket_addr).await;
    //                         crate::network::udp::reader_and_writer(
    //                             stream,
    //                             fib_clone,
    //                             // ebpf_tx,
    //                             local_to_net_rx,
    //                         ).await;
    //                     }
    //                 });
    //                 // send to fib an update
    //                 let channel_update_msg = FibStateChange {
    //                     action: FibChangeAction::ADD,
    //                     topic_gdp_name: topic_gdp_name,
    //                     // here is a little bit tricky:
    //                     //  to fib, it is the receiver
    //                     // it connects to a remote receiver
    //                     connection_type: direction_str_to_connection_type(flip_direction(direction).unwrap().as_str()),
    //                     forward_destination: Some(local_to_net_tx),
    //                     description: Some(format!(
    //                         "udp stream sending for topic_name {:?} to address {:?} direction {:?}",
    //                         topic_gdp_name, receiver_addr, direction
    //                     )),
    //                 };
    //                 let _ = channel_tx.send(channel_update_msg);
    //             }

    //         }
}

// protocol:
// sender_manager() 
// candidates <- init_candidates()
// append (GDPNAME, [candidates]) to {<topic_name>-sender}
// subscribe to {<topic_name>-receiver}

// on_new_connection_to_candidates():
// 	send "PING {ts}"; await response
// 	if PONG -> inform RIB a connectivity option 
	
// on_new_receiver():
// 	connect to candidates in receiver
// 	// sender needs to make sure receiver can receive it 
// 	send "PING {ts}"; await response 
// 	if PONG -> inform RIB a connectivity option 

pub async fn register_stream_sender(
    topic_gdp_name: GDPName, direction: String, fib_tx: UnboundedSender<GDPPacket>,
    channel_tx: UnboundedSender<FibStateChange>, interface: &str, config: fogrs_kcp::KcpConfig,
) {
    let redis_url = get_redis_url();
    let sender_key_name = format!("{}-{:}", topic_gdp_name, direction);
    let receiver_key_name = format!(
        "{}-{:}",
        topic_gdp_name,
        flip_direction(direction.as_str()).unwrap()
    );
    let thread_gdp_name = generate_random_gdp_name();

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


    let candidate_interfaces = gather_candidate_interfaces();
    let mut candidate_struct = CandidateStruct {
        thread_gdp_name: thread_gdp_name.clone(),
        candidates: vec![],
    };
    for interface_name in candidate_interfaces {
            // open a socket to the candidate
            let socket = Socket::new(Domain::IPV4, Type::DGRAM, None).unwrap();

            let tokio_socket = UdpSocket::from_std(socket.into()).unwrap();
            // Bind the socket to a specific interface
            bind_to_interface(&tokio_socket, interface).unwrap();
            // get stun address
            let sock_public_addr = match get_socket_stun(&tokio_socket).await {
                Ok(addr) => {
                    candidate_struct.candidates.push(addr);
                    addr
                },
                Err(err) => {
                    warn!("Address {:?} is not reachable as socket, error: {}", interface_name, err);
                    continue;
                }
            };
            // let _ = tokio::spawn(
            //     opening_side_socket_handler(
            //         topic_gdp_name.clone(),
            //         direction.clone(),
            //         fib_tx.clone(),
            //         channel_tx.clone(),
            //         tokio_socket,
            //         sock_public_addr,
            //         config.clone(),
            //     ).await;
            // ).await;
    }
    info!("candidates {:?}", candidate_struct);

    let receiver_candidates = get_entity_from_database(&redis_url, &receiver_key_name)
        .expect("Cannot get sender from database");
    info!(
        "get a list of receivers from KVS {:?}",
        receiver_candidates
    );

    // let _ = add_entity_to_database_as_transaction(
    //     &redis_url,
    //     &sender_key_name,
    //     serde_json::to_string(&candidate_struct).unwrap().as_str(),
    // );
    // info!(
    //     "registered {:?} with {:?}",
    //     topic_gdp_name, candidate_struct
    // );

    for receiver_candidate in receiver_candidates {
        let receiver_struct: CandidateStruct = serde_json::from_str(&receiver_candidate).unwrap();
        info!("receiver_struct {:?}", receiver_struct);
        for candidate_addr in receiver_struct.candidates {
            let latency = get_latency_for_remote_ip_addr_from_all_interfaces(candidate_addr).await;
            info!("latency {:?}", latency);
        }
    }

}

// protocol:
// receiving: determining which side should open the socket for a connection
// sending: selecting top K interfaces to send a message

// key: {<topic_name>-sender}, value: [a list of [candidates for a sender]]
// key: {<topic_name>-receiver}, value: [a list of [candidates for a receiver]]

// receiver_manager()
// candidates <- init_candidates()
// append (GDPNAME, [candidates]) to {<topic_name>-receiver}
// subscribe to {<topic_name>-sender}
// on_new_sender(): 
// 	connect to candidates in sender 
// 	await PING 
// on_new_connection():
// 	return with "PONG {ts}"

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CandidateStruct {
    thread_gdp_name: GDPName,
    candidates: Vec<SocketAddr>,
}

pub async fn register_stream_receiver(
    topic_gdp_name: GDPName, direction: String, fib_tx: UnboundedSender<GDPPacket>,
    channel_tx: UnboundedSender<FibStateChange>, interface: &str, config: fogrs_kcp::KcpConfig,
) {
    // let direction: &str = direction.as_str();
    let redis_url = get_redis_url();
    let receiver_key_name = format!("{}-{:}", topic_gdp_name, &direction);
    let sender_key_name = format!(
        "{}-{:}",
        topic_gdp_name,
        flip_direction(&direction).unwrap()
    );
    let thread_gdp_name = generate_random_gdp_name();

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

    let candidate_interfaces = gather_candidate_interfaces();
    let mut candidate_struct = CandidateStruct {
        thread_gdp_name: thread_gdp_name.clone(),
        candidates: vec![],
    };
    let fib_tx_clone = fib_tx.clone();
    let channel_tx_clone = channel_tx.clone();
    for interface_name in candidate_interfaces {
        let direction_clone = direction.clone();
        let fib_tx_clone = fib_tx_clone.clone(); 
        let channel_tx_clone = channel_tx_clone.clone();
        // open a socket to the candidate
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).unwrap();

        // Bind the socket to a specific interface
        info!("UDP socket is bound to {:?} with device name {}", socket.local_addr().unwrap(), interface_name);
        
        let tokio_socket = UdpSocket::from_std(socket.into()).unwrap();
        bind_to_interface(&tokio_socket, interface_name.as_str());
        // get stun address
        let sock_public_addr = match get_socket_stun(&tokio_socket).await {
            Ok(addr) => {
                candidate_struct.candidates.push(addr);
                addr
            },
            Err(err) => {
                warn!("Address {:?} is not reachable as socket, error: {}", interface_name, err);
                continue;
            }
        };
        let _ = tokio::spawn(
            async move{
                opening_side_socket_handler(
                    topic_gdp_name.clone(),
                    direction_clone.clone(),
                    fib_tx_clone,
                    channel_tx_clone,
                    tokio_socket,
                    sock_public_addr,
                    config.clone(),
                ).await;
            }
        );
    }
    info!("candidates {:?}", candidate_struct);

    // put value [candidates] to key {<topic_name>-receiver}
    let _ = add_entity_to_database_as_transaction(
        &redis_url,
        &receiver_key_name,
        serde_json::to_string(&candidate_struct).unwrap().as_str(),
    );
    info!(
        "registered {:?} with {:?}",
        topic_gdp_name, candidate_struct
    );

    tokio::select! {
        Some(message) = msgs.next() => {
            info!("msg {:?}", message);
            let received_operation = String::from_resp(message.unwrap()).unwrap();

            if received_operation != "lpush" {
                info!("the operation is not lpush, ignore");
                return;
            }
            let updated_senders = get_entity_from_database(&redis_url, &sender_key_name)
                .expect("Cannot get sender from database");
            info!("get a list of senders from KVS {:?}", updated_senders);

            }
        }
}

    // return;
    // let current_value_under_key = get_entity_from_database(&redis_url, &sender_key_name)
    //     .expect("Cannot get sender from database");
    // info!(
    //     "get a list of senders from KVS {:?}",
    //     current_value_under_key
    // );

    // let mut processed_senders = vec![];


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
                    let interface = "wlo1".to_string();//get_default_interface_name().unwrap();
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
                        // let interface = get_default_interface_name().unwrap();
                        let interface = "wlo1".to_string();//get_default_interface_name().unwrap();

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

}
