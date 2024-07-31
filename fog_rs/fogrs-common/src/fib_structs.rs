use crate::packet_structs::{GDPName, GDPPacket};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::sync::mpsc::UnboundedSender;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateStruct {
    pub thread_gdp_name: GDPName,
    pub candidates: Vec<SocketAddr>,
}

// Define your necessary structures and enums (e.g., GDPPacket, FibStateChange, etc.)

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct RosTopicStatus {
    pub action: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct RoutingManagerRequest {
    pub action: FibChangeAction,
    pub topic_name: String,
    pub topic_type: String,
    pub topic_qos: String,
    pub certificate: Vec<u8>,
    pub connection_type: Option<String>,
    pub communication_url: Option<String>,
}


// #[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize, Hash)]
// pub enum FibChangeAction {
//     ADD,
//     PAUSE,    // pausing the forwarding of the topic, keeping connections alive
//     PAUSEADD, // adding the entry to FIB, but keeps it paused
//     RESUME,   // resume a paused topic
//     DELETE,   // deleting a local topic interface and all its connections
//     RESPONSE,
// }

// #[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize, Hash)]
// pub enum FibConnectionType {
//     REQUEST,
//     RESPONSE,
//     SENDER,
//     RECEIVER,
//     BIDIRECTIONAL,
// }

// #[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize, Hash)]
// pub enum TopicStateInFIB {
//     RUNNING,
//     PAUSED,
//     DELETED,
// }

// #[derive(Debug)]
// pub struct FibStateChange {
//     pub action: FibChangeAction,
//     pub connection_type: FibConnectionType,
//     pub topic_gdp_name: GDPName,
//     pub forward_destination: Option<UnboundedSender<GDPPacket>>,
//     pub description: Option<String>,
// }

// #[derive(Debug)]
// pub struct FibConnection {
//     pub state: TopicStateInFIB,
//     pub connection_type: FibConnectionType,
//     pub tx: UnboundedSender<GDPPacket>,
//     pub description: Option<String>,
// }

// #[derive(Debug)]
// pub struct FIBState {
//     pub receivers: Vec<FibConnection>,
// }


#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize, Hash)]
pub enum FibChangeAction {
    ADD,
    PAUSE,    // pausing the forwarding of the topic, keeping connections alive
    PAUSEADD, // adding the entry to FIB, but keeps it paused
    RESUME,   // resume a paused topic
    DELETE,   // deleting a local topic interface and all its connections
    STATE,    // save the state of the topic
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize, Hash)]
pub enum FibConnectionType {
    SENDER,
    RECEIVER,
    REQUESTSENDER,
    REQUESTRECEIVER,
    RESPONSESENDER,
    RESPONSERECEIVER,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize, Hash)]
pub enum TopicStateInFIB {
    RUNNING,
    PAUSED,
    DELETED,
}

#[derive(Debug)]
pub struct FibStateChange {
    pub action: FibChangeAction,
    pub connection_type: FibConnectionType,
    pub topic_gdp_name: GDPName,
    pub forward_destination: Option<UnboundedSender<GDPPacket>>,
    pub interface: Option<String>,
    pub address: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug)]
pub struct FibConnection {
    pub state: TopicStateInFIB,
    pub connection_type: FibConnectionType,
    pub tx: UnboundedSender<GDPPacket>,
    pub description: Option<String>,
}

#[derive(Debug)]
pub struct FIBState {
    pub receivers: Vec<FibConnection>,
}
