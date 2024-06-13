pub mod udp;
// pub mod webrtc;

use std::net::SocketAddr;
use axum::async_trait;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
// use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use async_datachannel::DataStream;
use tokio::net::UdpSocket;
use fogrs_common::packet_structs::{GDPPacket, GDPHeaderInTransit, GDPName, GdpAction};


// Trait for managing different types of connections
#[async_trait]
pub trait ConnectionManager {
    async fn register_stream(&self, peer_to_dial: Option<String>) -> Result<(), std::io::Error>;
    async fn handle_data_stream(
        &self,
        ros_tx: UnboundedSender<GDPPacket>,
        rtc_rx: UnboundedReceiver<GDPPacket>,
    );
}

// Utility function for parsing packets
pub fn parse_header_payload_pairs(
    mut buffer: Vec<u8>,
) -> (
    Vec<(GDPHeaderInTransit, Vec<u8>)>,
    Option<(GDPHeaderInTransit, Vec<u8>)>,
) {
    let mut header_payload_pairs: Vec<(GDPHeaderInTransit, Vec<u8>)> = Vec::new();
    let default_gdp_header: GDPHeaderInTransit = GDPHeaderInTransit {
        action: GdpAction::Noop,
        destination: GDPName([0u8, 0, 0, 0]),
        source: GDPName([0u8, 0, 0, 0]),
        guid: GDPName([0u8, 0, 0, 0]),
        length: 0,
    };
    if buffer.is_empty() {
        return (header_payload_pairs, None);
    }
    loop {
        let header_and_remaining = buffer.splitn(2, |c| c == &0).collect::<Vec<_>>();
        let header_buf = header_and_remaining[0];
        let header: &str = std::str::from_utf8(header_buf).unwrap_or("");
        info!("received header json string: {:?}", header);

        if let Ok(gdp_header) = serde_json::from_str::<GDPHeaderInTransit>(header) {
            let remaining = header_and_remaining[1];//.unwrap_or(&[]);

            if gdp_header.length > remaining.len() {
                return (
                    header_payload_pairs,
                    Some((gdp_header, remaining.to_vec())),
                );
            } else if gdp_header.length == remaining.len() {
                header_payload_pairs.push((gdp_header, remaining.to_vec()));
                return (header_payload_pairs, None);
            } else {
                header_payload_pairs.push((gdp_header, remaining[..gdp_header.length].to_vec()));
                buffer = remaining[gdp_header.length..].to_vec();
            }
        } else {
            warn!("header is not complete, returning the remaining buffer");
            return (
                header_payload_pairs,
                Some((default_gdp_header, header_buf.to_vec())),
            );
        }
    }
}
