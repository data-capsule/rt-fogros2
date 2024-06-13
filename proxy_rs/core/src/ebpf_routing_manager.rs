use std::net::SocketAddr;

// use aya::{
//     include_bytes_aligned,
//     maps::HashMap,
//     programs::{ tc, SchedClassifier, TcAttachType },
//     Bpf,
// };
// use aya_log::BpfLogger;
use serde::{Deserialize, Serialize};

use fogrs_common::packet_structs::GDPName;

#[derive(Debug, Clone, Serialize, Deserialize)]

pub struct NewEbpfTopicRequest {
    pub name: String,
    pub gdp_name: GDPName,
    pub direction: String,
    pub sock_public_addr: SocketAddr,
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
