use std::net::{Ipv4Addr, SocketAddr};

use aya::{
    include_bytes_aligned,
    maps::HashMap,
    programs::{ tc, SchedClassifier, TcAttachType },
    Bpf,
};
use aya_log::BpfLogger;
use futures::StreamExt;
use log::{ info, warn };
use redis_async::{client, resp::FromResp};

use crate::{ db::{get_entity_from_database, get_redis_address_and_port, get_redis_url}, structs::GDPName, util::get_non_existent_ip_addr };
use crate::structs::{GDPPacket, GdpAction, Packet};
use crate::db::{
    add_entity_to_database_as_transaction,
    allow_keyspace_notification,
};

pub async fn register_stream(
    topic_gdp_name: GDPName,
    direction: String, // Sender or Receiver
    sock_public_addr: SocketAddr,
){
    let direction: &str = direction.as_str();
    let redis_url = get_redis_url();
    let _ = add_entity_to_database_as_transaction(
        &redis_url,
        format!("{:?}-{:}", topic_gdp_name, direction).as_str(),
        sock_public_addr.to_string().as_str(),
    );
    let receiver_topic = "GDPName([143, 157, 149, 87])-request-receiver";
    let redis_url = get_redis_url();
    let updated_receivers = get_entity_from_database(
        &redis_url,
        &receiver_topic
    ).expect("Cannot get receiver from database");
    info!("get a list of receivers from KVS {:?}", updated_receivers);


    
    let redis_addr_and_port = get_redis_address_and_port();
    let pubsub_con = client
        ::pubsub_connect(redis_addr_and_port.0, redis_addr_and_port.1).await
        .expect("Cannot connect to Redis");
    let redis_topic_stream_name: String = format!(
        "__keyspace@0__:{receiver_topic}",  
    );

    let mut msgs = pubsub_con.psubscribe(&redis_topic_stream_name).await.expect("Cannot subscribe to topic");

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
            }
            None => {
                info!("No message received");
            }
        }
    }
}

pub async fn ebpf_routing_manager() {
    // This will include your eBPF object file as raw bytes at compile-time and load it at
    // runtime. This approach is recommended for most real-world use cases. If you would
    // like to specify the eBPF program at runtime rather than at compile-time, you can
    // reach for `Bpf::load_file` instead.
    #[cfg(debug_assertions)]
    let mut bpf = Bpf::load(
        include_bytes_aligned!("../../target/bpfel-unknown-none/debug/tc-egress")
    ).unwrap();
    #[cfg(not(debug_assertions))]
    let mut bpf = Bpf::load(
        include_bytes_aligned!("../../target/bpfel-unknown-none/release/tc-egress")
    );
    if let Err(e) = BpfLogger::init(&mut bpf) {
        // This can happen if you remove all log statements from your eBPF program.
        warn!("failed to initialize eBPF logger: {}", e);
    }
    // error adding clsact to the interface if it is already added is harmless
    // the full cleanup can be done with 'sudo tc qdisc del dev eth0 clsact'.
    let _ = tc::qdisc_add_clsact("ens5");
    let program: &mut SchedClassifier = bpf.program_mut("tc_egress").unwrap().try_into().unwrap();
    program.load();
    program.attach("ens5", TcAttachType::Egress);

    warn!("interface attached!");
    // (1)
    let mut blocklist: HashMap<_, u32, u32> = HashMap::try_from(
        bpf.map_mut("BLOCKLIST").unwrap()
    ).unwrap();

    // (2)
    let block_addr: u32 = get_non_existent_ip_addr().into();

    // (3)
    blocklist.insert(block_addr, 0, 0);

    loop{
        // sleep 
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}
