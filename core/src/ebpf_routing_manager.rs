


use std::net::Ipv4Addr;

use aya::{
    include_bytes_aligned,
    maps::HashMap,
    programs::{tc, SchedClassifier, TcAttachType},
    Bpf,
};
use aya_log::BpfLogger;
use log::{info, warn};
use redis_async::client;

use crate::{db::get_redis_address_and_port, util::get_non_existent_ip_addr};


pub async fn ebpf_routing_manager() {

    // This will include your eBPF object file as raw bytes at compile-time and load it at
    // runtime. This approach is recommended for most real-world use cases. If you would
    // like to specify the eBPF program at runtime rather than at compile-time, you can
    // reach for `Bpf::load_file` instead.
    #[cfg(debug_assertions)]
    let mut bpf = Bpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/debug/tc-egress"
    )).unwrap();
    #[cfg(not(debug_assertions))]
    let mut bpf = Bpf::load(include_bytes_aligned!(
        "../../target/bpfel-unknown-none/release/tc-egress"
    ));
    if let Err(e) = BpfLogger::init(&mut bpf) {
        // This can happen if you remove all log statements from your eBPF program.
        warn!("failed to initialize eBPF logger: {}", e);
    }
    // error adding clsact to the interface if it is already added is harmless
    // the full cleanup can be done with 'sudo tc qdisc del dev eth0 clsact'.
    let _ = tc::qdisc_add_clsact("ens5");
    let program: &mut SchedClassifier =
        bpf.program_mut("tc_egress").unwrap().try_into().unwrap();
    program.load();
    program.attach("ens5", TcAttachType::Egress);

    warn!("interface attached!");
    // (1)
    let mut blocklist: HashMap<_, u32, u32> =
        HashMap::try_from(bpf.map_mut("BLOCKLIST").unwrap()).unwrap();

    // (2)
    let block_addr: u32 = get_non_existent_ip_addr().into();

    // (3)
    blocklist.insert(block_addr, 0, 0);

    let redis_addr_and_port = get_redis_address_and_port();
    let pubsub_con = client::pubsub_connect(redis_addr_and_port.0, redis_addr_and_port.1)
        .await
        .expect("Cannot connect to Redis");
    // let topic = format!("__keyspace@0__:{}", receiver_topic);
    // let mut msgs = pubsub_con
    //     .psubscribe(&topic)
    //     .await
    //     .expect("Cannot subscribe to topic");

    while true {
        std::thread::sleep(std::time::Duration::from_millis(10000));
    }

}
