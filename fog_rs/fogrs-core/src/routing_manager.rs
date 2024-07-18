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
use fogrs_signaling::Message;
use futures::StreamExt;
use librice::candidate;
use openssl::sign;
use redis_async::client;
use redis_async::resp::FromResp;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time;
use std::fmt::format;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::{TcpStream, UdpSocket};
use fogrs_common::fib_structs::CandidateStruct;
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
    // let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).unwrap();

    // let socket = UdpSocket::from_std(socket.into()).unwrap();
    // // TODO: bind to interface
    // bind_to_interface(&socket, interface_name).unwrap();

    let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
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

        let latency = send_ping_from_interface(&interface_name, remote_ip_addr).await;
        if latency.is_ok() {
            info!("ping from interface {} to remote ip address {} is successful", interface_name, remote_ip_addr);
            latencies.push((interface_name, start_time.elapsed())); 
        }
        else{
            warn!("ping from interface {} to remote ip address {} is failed, error: {}", interface_name, remote_ip_addr, latency.unwrap_err());
        }
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
    // topic_gdp_name: GDPName, direction: String,
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
        
        // let direction = direction.clone();
        let channel_tx_clone = channel_tx_clone.clone();
        let (local_to_net_tx, local_to_net_rx) = mpsc::unbounded_channel();  

        error!("TODO: implement the logic for the remote sender, need to know topic name and direction");      

        tokio::spawn(async move {
            crate::network::kcp::reader_and_writer(stream, fib_tx_clone, local_to_net_rx)
                 .await;
        });
        
        // tokio::spawn(async move {
        //     let channel_update_msg = FibStateChange {
        //         action: FibChangeAction::ADD,
        //         topic_gdp_name: topic_gdp_name,
        //         // connection_type: direction_str_to_connection_type(direction.as_str()), // it connects from a remote sender
        //         // here is a little bit tricky: 
        //         //  to fib, it is the receiver
        //         // it connects to a remote receiver
        //         connection_type: direction_str_to_connection_type(flip_direction(direction.as_str()).unwrap().as_str()),
        //         forward_destination: Some(local_to_net_tx),
        //         description: Some(format!(
        //             "udp stream connecting to remote sender for topic_name {:?} bind to address {:?} from {:?} direction {:?}",
        //             topic_gdp_name, sock_public_addr, peer_addr, direction,
        //         )),
        //     };
        //     channel_tx_clone
        //         .send(channel_update_msg)
        //         .expect("Cannot send channel update message");
        //     crate::network::kcp::reader_and_writer(stream, fib_tx_clone, local_to_net_rx)
        //         .await;
        //     }
        // );
        }
    });
}





pub async fn register_stream_sender(
    topic_gdp_name: GDPName, direction: String, fib_tx: UnboundedSender<GDPPacket>,
    channel_tx: UnboundedSender<FibStateChange>, interface: &str, config: fogrs_kcp::KcpConfig,
) {
    // let direction: &str = direction.as_str();

    let mut signaling_stream = TcpStream::connect("127.0.0.1:8080").await.unwrap();

    let thread_gdp_name = generate_random_gdp_name();
    let candidate_interfaces = gather_candidate_interfaces();
    let mut candidate_struct = CandidateStruct {
        thread_gdp_name: thread_gdp_name.clone(),
        candidates: vec![],
    };
    let fib_tx_clone = fib_tx.clone();
    let channel_tx_clone = channel_tx.clone();
    for interface_name in candidate_interfaces {
        // let direction_clone = direction.clone();
        let fib_tx_clone = fib_tx_clone.clone(); 
        let channel_tx_clone = channel_tx_clone.clone();

        let tokio_socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        bind_to_interface(&tokio_socket, interface_name.as_str()).unwrap();
        
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
                    // topic_gdp_name.clone(),
                    // direction_clone.clone(),
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

    let request = Message {
        command: "SUBSCRIBE".to_string(),
        topic: format!("{}-{:}", topic_gdp_name, flip_direction(&direction).unwrap()),
        data: None,
    };
    let request = serde_json::to_string(&request).unwrap();
    signaling_stream.write_all(request.as_bytes()).await.unwrap();
    // signaling_stream.flush().await.unwrap();

    let mut publish_stream = TcpStream::connect("127.0.0.1:8080").await.unwrap();
    let request = Message {
        command: "PUBLISH".to_string(),
        topic: format!("{}-{:}", topic_gdp_name, &direction),
        data: Some(candidate_struct),
    };
    let mut request = serde_json::to_string(&request).unwrap();
    // request.push('\n');
    publish_stream.write_all(request.as_bytes()).await;
    info!("sent to signaling server {:?}", request);
    publish_stream.flush().await.unwrap();
    publish_stream.shutdown().await.unwrap();


    let mut buffer = [0; 1024];
    loop {
        info!("waiting for message");
        let n = signaling_stream.read(&mut buffer).await.unwrap();
        if n == 0 {
            break;
        }
        let str_buf = String::from_utf8_lossy(&buffer[..n]);
        println!("{}", str_buf);
        // let receiver_candidates_buf = buffer;
        // let receiver_candidate = serde_json::from_slice(&buffer).unwrap();

        let receiver_struct: CandidateStruct = serde_json::from_str(&str_buf).unwrap();
        info!("receiver_struct {:?}", receiver_struct);
        for candidate_addr in receiver_struct.candidates {
            let interface_to_latency = get_latency_for_remote_ip_addr_from_all_interfaces(candidate_addr).await;
            info!("interface_to_latency {:?}", interface_to_latency);
            
            // forward from the interface to 

            for (interface_name, latency) in interface_to_latency {
                let direction = direction.clone();

                let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
                bind_to_interface(&socket, interface_name.as_str()).unwrap();

                let _ = fogrs_kcp::KcpStream::connect(&config, candidate_addr).await.unwrap();
                let (local_to_net_tx, local_to_net_rx) = mpsc::unbounded_channel();
                let fib_clone = fib_tx.clone();
                tokio::spawn(async move {
                    let stream = fogrs_kcp::KcpStream::connect(&config, candidate_addr).await.unwrap();
                    info!("connected to {:?}", candidate_addr);
                    crate::network::kcp::reader_and_writer(
                        stream,
                        fib_clone,
                        // ebpf_tx,
                        local_to_net_rx,
                    ).await;
                });
                // send to fib an update
                let channel_update_msg = FibStateChange {
                    action: FibChangeAction::ADD,
                    topic_gdp_name: topic_gdp_name,
                    // here is a little bit tricky:
                    //  to fib, it is the receiver
                    // it connects to a remote receiver
                    connection_type: direction_str_to_connection_type(flip_direction(direction.as_str()).unwrap().as_str()),
                    forward_destination: Some(local_to_net_tx),
                    description: Some(format!(
                        "udp stream sending for topic_name {:?} to address {:?} direction {:?}",
                        topic_gdp_name, candidate_addr, direction
                    )),
                };
                let _ = channel_tx.send(channel_update_msg);
            }
        }
    }
}



pub async fn register_stream_receiver(
    topic_gdp_name: GDPName, direction: String, fib_tx: UnboundedSender<GDPPacket>,
    channel_tx: UnboundedSender<FibStateChange>, interface: &str, config: fogrs_kcp::KcpConfig,
) {
    // let direction: &str = direction.as_str();

    let mut signaling_stream = TcpStream::connect("127.0.0.1:8080").await.unwrap();


    let thread_gdp_name = generate_random_gdp_name();
    let candidate_interfaces = gather_candidate_interfaces();
    let mut candidate_struct = CandidateStruct {
        thread_gdp_name: thread_gdp_name.clone(),
        candidates: vec![],
    };
    let fib_tx_clone = fib_tx.clone();
    let channel_tx_clone = channel_tx.clone();
    for interface_name in candidate_interfaces {
        // let direction_clone = direction.clone();
        let fib_tx_clone = fib_tx_clone.clone(); 
        let channel_tx_clone = channel_tx_clone.clone();

        let tokio_socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        bind_to_interface(&tokio_socket, interface_name.as_str()).unwrap();
        
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
                    // topic_gdp_name.clone(),
                    // direction_clone.clone(),
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

    // info!(
    //     "Attempting to subscribe to topic: {}",
    //     redis_topic_stream_name
    // );
    let request = Message {
        command: "SUBSCRIBE".to_string(),
        topic: format!("{}-{:}", topic_gdp_name, flip_direction(&direction).unwrap()),
        data: None,
    };
    let request = serde_json::to_string(&request).unwrap();
    signaling_stream.write_all(request.as_bytes()).await.unwrap();
    // signaling_stream.flush().await.unwrap();

    let mut publish_stream = TcpStream::connect("127.0.0.1:8080").await.unwrap();
    let request = Message {
        command: "PUBLISH".to_string(),
        topic: format!("{}-{:}", topic_gdp_name, &direction),
        data: Some(candidate_struct),
    };
    let mut request = serde_json::to_string(&request).unwrap();
    // request.push('\n');
    publish_stream.write_all(request.as_bytes()).await;
    info!("sent to signaling server {:?}", request);
    publish_stream.flush().await.unwrap();
    publish_stream.shutdown().await.unwrap();


    let mut buffer = [0; 1024];
    loop {
        info!("waiting for message");
        let n = signaling_stream.read(&mut buffer).await.unwrap();
        if n == 0 {
            break;
        }
        let str_buf = String::from_utf8_lossy(&buffer[..n]);
        println!("{}", str_buf);
        // let receiver_candidates_buf = buffer;
        // let receiver_candidate = serde_json::from_slice(&buffer).unwrap();

        let receiver_struct: CandidateStruct = serde_json::from_str(&str_buf).unwrap();
        info!("receiver_struct {:?}", receiver_struct);
        for candidate_addr in receiver_struct.candidates {
            let interface_to_latency = get_latency_for_remote_ip_addr_from_all_interfaces(candidate_addr).await;
            info!("interface_to_latency {:?}", interface_to_latency);
            
            // forward from the interface to 

            for (interface_name, latency) in interface_to_latency {
                let direction = direction.clone();

                let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
                bind_to_interface(&socket, interface_name.as_str()).unwrap();

                let _ = fogrs_kcp::KcpStream::connect(&config, candidate_addr).await.unwrap();
                let (local_to_net_tx, local_to_net_rx) = mpsc::unbounded_channel();
                let fib_clone = fib_tx.clone();
                tokio::spawn(async move {
                    let stream = fogrs_kcp::KcpStream::connect(&config, candidate_addr).await.unwrap();
                    info!("connected to {:?}", candidate_addr);
                    crate::network::kcp::reader_and_writer(
                        stream,
                        fib_clone,
                        // ebpf_tx,
                        local_to_net_rx,
                    ).await;
                });
                // send to fib an update
                let channel_update_msg = FibStateChange {
                    action: FibChangeAction::ADD,
                    topic_gdp_name: topic_gdp_name,
                    // here is a little bit tricky:
                    //  to fib, it is the receiver
                    // it connects to a remote receiver
                    connection_type: direction_str_to_connection_type(flip_direction(direction.as_str()).unwrap().as_str()),
                    forward_destination: Some(local_to_net_tx),
                    description: Some(format!(
                        "udp stream sending for topic_name {:?} to address {:?} direction {:?}",
                        topic_gdp_name, candidate_addr, direction
                    )),
                };
                let _ = channel_tx.send(channel_update_msg);
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
