use axum::async_trait;
use fogrs_common::packet_structs::construct_gdp_packet_with_guid;
use fogrs_common::packet_structs::GDPName;
use fogrs_common::packet_structs::Packet;
use tokio::net::UdpSocket;
use librice::stun::attribute::*;
use librice::stun::message::*;
use std::net::SocketAddr;
use std::str::FromStr;
use crate::ebpf_routing_manager::register_stream;
use crate::network::parse_header_payload_pairs;
use crate::util::get_non_existent_ip_addr;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
// use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use async_datachannel::DataStream;
use fogrs_common::packet_structs::{GDPPacket, GDPHeaderInTransit, GdpAction};

const UDP_BUFFER_SIZE: usize = 1500;

use super::ConnectionManager;

pub struct UdpManager {
    socket: UdpSocket,
    peer_addr: Option<SocketAddr>,
}

impl UdpManager {
    pub async fn new() -> Self {
        let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        let peer_addr = None;

        UdpManager { socket, peer_addr }
    }

    async fn get_socket_stun(&self) -> Result<SocketAddr, std::io::Error> {
        let ice_server = SocketAddr::from_str("3.18.194.127:3478").unwrap();
        let mut msg = Message::new_request(BINDING);
        msg.add_fingerprint().unwrap();

        self.udp_ice_get(msg, ice_server).await
    }

    async fn udp_ice_get(
        &self,
        out: Message,
        to: SocketAddr,
    ) -> Result<SocketAddr, std::io::Error> {
        info!("generated to {}", out);
        let buf = out.to_bytes();
        self.socket.send_to(&buf, to).await?;
        let mut buf = [0; 1500];
        let (amt, _) = self.socket.recv_from(&mut buf).await?;
        let buf = &buf[..amt];
        let msg = Message::from_bytes(buf)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid message"))?;
        info!("got message {:?}", msg);

        parse_response(msg)
    }
}

fn parse_response(response: Message) -> Result<SocketAddr, std::io::Error> {
    if Message::check_attribute_types(&response, &[XOR_MAPPED_ADDRESS, FINGERPRINT], &[
        XOR_MAPPED_ADDRESS,
    ])
    .is_some()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Required attributes not found in response",
        ));
    }
    if response.has_class(MessageClass::Success) {
        let mapped_address = response
            .attribute::<XorMappedAddress>(XOR_MAPPED_ADDRESS)
            .unwrap();
        let visible_addr = mapped_address.addr(response.transaction_id());
        info!("found visible address {:?}", visible_addr);
        Ok(visible_addr)
    } else {
        warn!("got error response {:?}", response);
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Error response",
        ))
    }
}

#[async_trait]
impl ConnectionManager for UdpManager {
    async fn register_stream(&self, peer_to_dial: Option<String>) -> Result<(), std::io::Error> {
        let sock_public_addr = self.get_socket_stun().await?;
        info!("UDP socket is bound to {:?}", sock_public_addr);

        let _ = tokio::spawn(register_stream(
            GDPName::default(), // Placeholder, replace with actual GDPName
            "direction".to_string(), // Placeholder, replace with actual direction
            sock_public_addr,
        ));

        Ok(())
    }

    async fn handle_data_stream(
        &self,
        ros_tx: UnboundedSender<GDPPacket>,
        mut rtc_rx: UnboundedReceiver<GDPPacket>,
    ) {
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
                Ok((receiving_buf_size, _)) = self.socket.recv_from(&mut receiving_buf) => {
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

                    let destination_ip = get_non_existent_ip_addr();
                    let destination = SocketAddr::new(std::net::IpAddr::V4(destination_ip), 8888);
                    header_string.push(0u8 as char);
                    let header_string_payload = header_string.as_bytes();
                    if self.socket.send_to(&header_string_payload[..header_string_payload.len()], destination).await.is_err() {
                        warn!("The connection is closed while sending header");
                        break;
                    }

                    if let Some(payload) = pkt_to_forward.payload {
                        info!("the payload length is {}", payload.len());
                        if self.socket.send_to(&payload[..payload.len()], destination).await.is_err() {
                            warn!("The connection is closed while sending payload");
                            break;
                        }
                    }

                    if let Some(name_record) = pkt_to_forward.name_record {
                        let name_record_string = serde_json::to_string(&name_record).unwrap();
                        let name_record_buffer = name_record_string.as_bytes();
                        info!("the name record length is {}", name_record_buffer.len());
                        if self.socket.send_to(&name_record_buffer[..name_record_buffer.len()], destination).await.is_err() {
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
