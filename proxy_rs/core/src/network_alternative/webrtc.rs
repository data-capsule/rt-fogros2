use axum::async_trait;
use fogrs_common::packet_structs::construct_gdp_packet_with_guid;
use fogrs_common::packet_structs::GDPName;
use tokio::net::UdpSocket;
use librice::stun::attribute::*;
use librice::stun::message::*;
use tokio::sync::mpsc;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Mutex;
use crate::ebpf_routing_manager::register_stream;
use crate::network::parse_header_payload_pairs;
use crate::util::get_non_existent_ip_addr;
use crate::util::get_signaling_server_address;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
// use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use async_datachannel::DataStream;
use fogrs_common::packet_structs::{GDPPacket, GDPHeaderInTransit, GdpAction};

use async_datachannel::{DataStream, Message, PeerConnection, RtcConfig};
use async_tungstenite::{tokio::connect_async, tungstenite};
use futures::{
    io::{AsyncReadExt, AsyncWriteExt},
    SinkExt, StreamExt};

#[derive(Debug, Serialize, Deserialize)]
struct SignalingMessage {
    id: String,
    payload: Message,
}

pub struct WebRTCManager {
    ice_servers: Vec<String>,
    signaling_uri: String,
    peer_id: Arc<Mutex<Option<String>>>,
}

impl WebRTCManager {
    pub fn new(my_id: &str) -> Self {
        let ice_servers = vec!["stun:stun.l.google.com:19302".to_string()];
        let signaling_uri = format!("{}/{}", get_signaling_server_address(), my_id);
        let peer_id = Arc::new(Mutex::new(None));

        WebRTCManager {
            ice_servers,
            signaling_uri,
            peer_id,
        }
    }

    async fn handle_signaling_outbound(
        mut write: futures::stream::SplitSink<async_tungstenite::WebSocketStream<async_tungstenite::tokio::ConnectStream>, tungstenite::Message>,
        mut rx_sig_outbound: mpsc::Receiver<Message>,
        peer_id: Arc<Mutex<Option<String>>>,
    ) {
        while let Some(m) = rx_sig_outbound.next().await {
            if let Some(ref id) = *peer_id.lock() {
                let message = SignalingMessage {
                    payload: m,
                    id: id.clone(),
                };
                let s = serde_json::to_string(&message).unwrap();
                info!("Sending {:?}", s);
                if write.send(tungstenite::Message::text(s)).await.is_err() {
                    error!("Error sending message");
                    break;
                }
            }
        }
    }

    async fn handle_signaling_inbound(
        mut read: futures::stream::SplitStream<async_tungstenite::WebSocketStream<async_tungstenite::tokio::ConnectStream>>,
        mut tx_sig_inbound: mpsc::Sender<Message>,
        peer_id: Arc<Mutex<Option<String>>>,
    ) {
        while let Some(Ok(m)) = read.next().await {
            info!("Received {:?}", m);
            if let Ok(value) = match m {
                tungstenite::Message::Text(t) => serde_json::from_str::<serde_json::Value>(&t),
                tungstenite::Message::Binary(b) => serde_json::from_slice(&b[..]),
                _ => Err(serde_json::Error::custom("Unsupported message type")),
            } {
                if let Ok(message) = serde_json::from_value::<SignalingMessage>(value) {
                    info!("Parsed message {:?}", message);
                    *peer_id.lock() = Some(message.id.clone());
                    if tx_sig_inbound.send(message.payload).await.is_err() {
                        panic!("Failed to send message to channel");
                    }
                }
            }
        }
    }
}

#[async_trait]
impl ConnectionManager for WebRTCManager {
    async fn register_stream(&self, peer_to_dial: Option<String>) -> Result<(), std::io::Error> {
        let conf = RtcConfig::new(&self.ice_servers);
        let (tx_sig_outbound, mut rx_sig_outbound) = mpsc::channel(32);
        let (mut tx_sig_inbound, rx_sig_inbound) = mpsc::channel(32);
        let listener = PeerConnection::new(&conf, (tx_sig_outbound, rx_sig_inbound)).unwrap();

        let signaling_uri = self.signaling_uri.clone();
        let (mut write, mut read) = connect_async(&signaling_uri).await.unwrap().0.split();
        let peer_id = self.peer_id.clone();
        if let Some(ref peer) = peer_to_dial {
            *peer_id.lock() = Some(peer.clone());
        }

        tokio::spawn(Self::handle_signaling_outbound(write, rx_sig_outbound, peer_id.clone()));
        tokio::spawn(Self::handle_signaling_inbound(read, tx_sig_inbound, peer_id.clone()));

        if peer_to_dial.is_some() {
            info!("Dialing {:?}", peer_to_dial);
            listener.dial("connection").await.map(|_| ())
        } else {
            info!("Accepting connection");
            listener.accept().await.map(|_| ())
        }
    }

    async fn handle_data_stream(
        &self,
        ros_tx: UnboundedSender<GDPPacket>,
        mut rtc_rx: UnboundedReceiver<GDPPacket>,
    ) {
        let mut stream = self.register_stream(None).await.unwrap();

        let mut need_more_data_for_previous_header = false;
        let mut remaining_gdp_header: GDPHeaderInTransit = GDPHeaderInTransit {
            action: GdpAction::Noop,
            destination: GDPName([0u8, 0, 0, 0]),
            source: GDPName([0u8, 0, 0, 0]),
            guid: GDPName([0u8, 0, 0, 0]),
            length: 0,
        };
        let mut remaining_gdp_payload: Vec<u8> = vec![];
        let mut reset_counter = 0;

        loop {
            let mut receiving_buf = vec![0u8; UDP_BUFFER_SIZE];
            tokio::select! {
                Ok(receiving_buf_size) = stream.read(&mut receiving_buf) => {
                    let mut receiving_buf = receiving_buf[..receiving_buf_size].to_vec();
                    info!("read {} bytes", receiving_buf_size);

                    let mut header_payload_pair = vec![];

                    if need_more_data_for_previous_header {
                        let read_payload_size = remaining_gdp_payload.len() + receiving_buf_size;
                        if remaining_gdp_header.action == GdpAction::Noop {
                            warn!("last time it has incomplete buffer to complete, the action is Noop.");
                            remaining_gdp_payload.append(&mut receiving_buf[..receiving_buf_size].to_vec());
                            receiving_buf = remaining_gdp_payload.clone();
                            reset_counter += 1;
                            if reset_counter > 5 {
                                error!("unable to match the buffer, reset the connection");
                                receiving_buf = vec![];
                                remaining_gdp_payload = vec![];
                                reset_counter = 0;
                            }
                        } else if read_payload_size < remaining_gdp_header.length {
                            info!("more data to read. Current {}, need {}, expect {}", read_payload_size, remaining_gdp_header.length, remaining_gdp_header.length - read_payload_size);
                            remaining_gdp_payload.append(&mut receiving_buf[..receiving_buf_size].to_vec());
                            continue;
                        } else if read_payload_size == remaining_gdp_header.length {
                            remaining_gdp_payload.append(&mut receiving_buf[..receiving_buf_size].to_vec());
                            header_payload_pair.push((remaining_gdp_header, remaining_gdp_payload.clone()));
                            receiving_buf = vec![];
                        } else {
                            warn!("The packet is overflowed!!! read_payload_size {}, remaining_gdp_header.length {}, remaining_gdp_payload.len() {}, receiving_buf_size {}", read_payload_size, remaining_gdp_header.length, remaining_gdp_payload.len(), receiving_buf_size);
                            let num_remaining = remaining_gdp_header.length - remaining_gdp_payload.len();
                            remaining_gdp_payload.append(&mut receiving_buf[..num_remaining].to_vec());
                            header_payload_pair.push((remaining_gdp_header, remaining_gdp_payload.clone()));
                            receiving_buf = receiving_buf[num_remaining..].to_vec();
                        }
                    }

                    let (mut processed_gdp_packets, processed_remaining_header) = parse_header_payload_pairs(receiving_buf.to_vec());
                    header_payload_pair.append(&mut processed_gdp_packets);
                    for (header, payload) in header_payload_pair {
                        let deserialized = header;
                        info!("the total received payload with size {} with gdp header length {}", payload.len(), header.length);
                        let packet = construct_gdp_packet_with_guid(deserialized.action, deserialized.destination, deserialized.source, payload, deserialized.guid);
                        if ros_tx.send(packet).is_err() {
                            warn!("request is being handled by another connection");
                        }
                    }

                    match processed_remaining_header {
                        Some((header, payload)) => {
                            remaining_gdp_header = header;
                            remaining_gdp_payload = payload;
                            need_more_data_for_previous_header = true;
                        },
                        None => {
                            need_more_data_for_previous_header = false;
                            remaining_gdp_payload = vec![];
                        }
                    }
                },
                Some(pkt_to_forward) = rtc_rx.recv() => {
                    let transit_header = pkt_to_forward.get_header();
                    let mut header_string = serde_json::to_string(&transit_header).unwrap();
                    info!("the header size is {}", header_string.len());
                    info!("the header to sent is {}", header_string);

                    header_string.push(0u8 as char);
                    let header_string_payload = header_string.as_bytes();
                    if stream.write_all(header_string_payload).await.is_err() {
                        warn!("The connection is closed while sending header");
                        break;
                    }

                    if let Some(payload) = pkt_to_forward.payload {
                        info!("the payload length is {}", payload.len());
                        if stream.write_all(&payload).await.is_err() {
                            warn!("The connection is closed while sending payload");
                            break;
                        }
                    }

                    if let Some(name_record) = pkt_to_forward.name_record {
                        let name_record_string = serde_json::to_string(&name_record).unwrap();
                        let name_record_buffer = name_record_string.as_bytes();
                        info!("the name record length is {}", name_record_buffer.len());
                        if stream.write_all(&name_record_buffer).await.is_err() {
                            warn!("The connection is closed while sending name record");
                            break;
                        }
                    }
                }
                else => {
                    info!("The connection is closed");
                    break;
                },
            }
        }
    }
}
