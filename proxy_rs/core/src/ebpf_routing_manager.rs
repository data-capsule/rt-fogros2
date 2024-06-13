use std::net::{Ipv4Addr, SocketAddr};

// use aya::{
//     include_bytes_aligned,
//     maps::HashMap,
//     programs::{ tc, SchedClassifier, TcAttachType },
//     Bpf,
// };
// use aya_log::BpfLogger;
use futures::StreamExt;
use log::{ info, warn };
use redis_async::{client, resp::FromResp};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedSender;

use crate::{ db::{get_entity_from_database, get_redis_address_and_port, get_redis_url}, util::get_non_existent_ip_addr };
use fogrs_common::packet_structs::{GDPPacket, GdpAction, Packet, GDPName};
use crate::db::{
    add_entity_to_database_as_transaction,
    allow_keyspace_notification,
};

# [derive(Debug, Clone, Serialize, Deserialize)]

pub struct NewEbpfTopicRequest {
    pub name: String,
    pub gdp_name: GDPName,
    pub direction: String,
    pub sock_public_addr: SocketAddr,
}

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

pub async fn register_stream(
    topic_gdp_name: GDPName,
    direction: String, 
    sock_public_addr: SocketAddr,
    // ebpf_tx: UnboundedSender<NewEbpfTopicRequest>,
){
    let direction: &str = direction.as_str();
    let redis_url = get_redis_url();
    let _ = add_entity_to_database_as_transaction(
        &redis_url,
        format!("{:?}-{:}", topic_gdp_name, direction).as_str(),
        sock_public_addr.to_string().as_str(),
    );
    info!("registered {:?} with {:?}", topic_gdp_name, sock_public_addr);

    let receiver_topic = format!("{:?}-{:}", topic_gdp_name, flip_direction(direction).unwrap());
    let redis_url = get_redis_url();
    let updated_receivers = get_entity_from_database(
        &redis_url,
        &receiver_topic
    ).expect("Cannot get receiver from database");
    info!("get a list of {:?} from KVS {:?}", flip_direction(direction), updated_receivers);
    
    if updated_receivers.len() != 0 {
        let receiver_addr = updated_receivers[0].clone();
        let receiver_socket_addr: SocketAddr = receiver_addr.parse().expect("Failed to parse receiver address");
        let ebpf_topic_request = NewEbpfTopicRequest {
            name: "new_topic".to_string(),
            gdp_name: topic_gdp_name.clone(),
            direction: direction.to_string(),
            sock_public_addr: receiver_socket_addr,
        };
        // ebpf_tx.send(ebpf_topic_request).unwrap();
    }



    // TODO: fix following code later, assume listener start before writer
    let redis_addr_and_port = get_redis_address_and_port();
    let pubsub_con = client
        ::pubsub_connect(redis_addr_and_port.0, redis_addr_and_port.1).await
        .expect("Cannot connect to Redis");
    let redis_topic_stream_name: String = format!(
        "__keyspace@0__:{}", receiver_topic
    );
    allow_keyspace_notification(&redis_url).expect("Cannot allow keyspace notification");
    let mut msgs = pubsub_con.psubscribe(&redis_topic_stream_name).await.expect("Cannot subscribe to topic");
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
                let updated_receivers = get_entity_from_database(
                    &redis_url,
                    &receiver_topic
                ).expect("Cannot get receiver from database");
                info!("get a list of receivers from KVS {:?}", updated_receivers);

                let ebpf_topic_request = NewEbpfTopicRequest {
                    name: "new_topic".to_string(),
                    gdp_name: topic_gdp_name.clone(),
                    direction: direction.to_string(),
                    sock_public_addr: sock_public_addr.clone(),
                };
                // ebpf_tx.send(ebpf_topic_request).unwrap();
            }
            None => {
                info!("No message received");
            }
        }
    }
}

// pub async fn ebpf_routing_manager(
//     mut ebpf_rx: tokio::sync::mpsc::UnboundedReceiver<NewEbpfTopicRequest>,
// ) {
//     // This will include your eBPF object file as raw bytes at compile-time and load it at
//     // runtime. This approach is recommended for most real-world use cases. If you would
//     // like to specify the eBPF program at runtime rather than at compile-time, you can
//     // reach for `Bpf::load_file` instead.
//     #[cfg(debug_assertions)]
//     let mut bpf = Bpf::load(
//         include_bytes_aligned!("../../target/bpfel-unknown-none/debug/tc-egress")
//     ).unwrap();
//     #[cfg(not(debug_assertions))]
//     let mut bpf = Bpf::load(
//         include_bytes_aligned!("../../target/bpfel-unknown-none/release/tc-egress")
//     );
//     if let Err(e) = BpfLogger::init(&mut bpf) {
//         // This can happen if you remove all log statements from your eBPF program.
//         warn!("failed to initialize eBPF logger: {}", e);
//     }

//     let interface = "ens5";
//     // error adding clsact to the interface if it is already added is harmless
//     // the full cleanup can be done with 'sudo tc qdisc del dev eth0 clsact'.
//     let _ = tc::qdisc_add_clsact(interface);
//     let program: &mut SchedClassifier = bpf.program_mut("tc_egress").unwrap().try_into().unwrap();
//     program.load();
//     program.attach(interface, TcAttachType::Egress);

//     warn!("interface attached!");
//     // (1)
//     let mut blocklist: HashMap<_, u32, u16> = HashMap::try_from(
//         bpf.map_mut("BLOCKLIST").unwrap()
//     ).unwrap();

//     // (2)
//     let block_addr: u32 = get_non_existent_ip_addr().into();

//     loop{
//         tokio::select! {
//             Some(topic) = ebpf_rx.recv() => {
//                 info!("received {:?}", topic);
//                 let port = topic.sock_public_addr.port();
//                 blocklist.insert(block_addr, port, 0);
//             }
//         }
//     }
// }
