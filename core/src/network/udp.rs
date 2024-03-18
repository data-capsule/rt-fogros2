use std::sync::Arc;

use crate::ebpf_routing_manager::{register_stream, NewEbpfTopicRequest};
use crate::pipeline::construct_gdp_packet_with_guid;
use crate::structs::GDPHeaderInTransit;
use crate::structs::{generate_random_gdp_name, GDPName};
use crate::structs::{GDPPacket, GdpAction, Packet};
use crate::util::{get_non_existent_ip_addr, get_signaling_server_address};
use tokio::net::UdpSocket;
use std::str::FromStr;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
const UDP_BUFFER_SIZE: usize = 1748000; // 17kb

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::info;
use std::net::{SocketAddr};
use librice::candidate::TransportType;
use librice::stun::attribute::*;
use librice::stun::message::*;


// use utils::app_config::AppConfig;

/// parse the header of the packet using the first null byte as delimiter
/// return a vector of (header, payload) pairs if the header is complete
/// return the remaining (header, payload) pairs if the header is incomplete
pub fn parse_header_payload_pairs(
    mut buffer: Vec<u8>,
) -> (
    Vec<(GDPHeaderInTransit, Vec<u8>)>,
    Option<(GDPHeaderInTransit, Vec<u8>)>,
) {
    let mut header_payload_pairs: Vec<(GDPHeaderInTransit, Vec<u8>)> = Vec::new();
    // TODO: get it to default trace later
    let default_gdp_header: GDPHeaderInTransit = GDPHeaderInTransit {
        action: GdpAction::Noop,
        destination: GDPName([0u8, 0, 0, 0]),
        source: GDPName([0u8, 0, 0, 0]),
        guid: GDPName([0u8, 0, 0, 0]),
        length: 0, // doesn't have any payload
    };
    if buffer.len() == 0 {
        return (header_payload_pairs, None);
    }
    loop {
        // parse the header
        // use the first null byte \0 as delimiter
        // split to the first \0 as delimiter
        let header_and_remaining = buffer.splitn(2, |c| c == &0).collect::<Vec<_>>();
        let header_buf = header_and_remaining[0];
        let header: &str = std::str::from_utf8(header_buf).unwrap();
        info!("received header json string: {:?}", header);
        let gdp_header_parsed = serde_json::from_str::<GDPHeaderInTransit>(header);
        if gdp_header_parsed.is_err() {
            // if the header is not complete, return the remaining
            warn!("header is not complete, return the remaining");
            return (
                header_payload_pairs,
                Some((default_gdp_header, header_buf.to_vec())),
            );
        }
        let gdp_header = gdp_header_parsed.unwrap();
        let remaining = header_and_remaining[1];

        if gdp_header.length > remaining.len() {
            // if the payload is not complete, return the remaining
            return (header_payload_pairs, Some((gdp_header, remaining.to_vec())));
        } else if gdp_header.length == remaining.len() {
            // if the payload is complete, return the pair
            header_payload_pairs.push((gdp_header, remaining.to_vec()));
            return (header_payload_pairs, None);
        } else {
            // if the payload is longer than the remaining, continue to parse
            header_payload_pairs.push((gdp_header, remaining[..gdp_header.length].to_vec()));
            buffer = remaining[gdp_header.length..].to_vec();
        }
    }
}

/// Works with the signalling server from https://github.com/paullouisageneau/libdatachannel/tree/master/examples/signaling-server-rust
/// Start two shells
/// 1. RUST_LOG=debug cargo run --example smoke -- ws://127.0.0.1:8000 other_peer
/// 2. RUST_LOG=debug cargo run --example smoke -- ws://127.0.0.1:8000 initiator other_peer


fn parse_response(response: Message) -> Result<SocketAddr, std::io::Error> {
    if Message::check_attribute_types(
        &response,
        &[XOR_MAPPED_ADDRESS, FINGERPRINT],
        &[XOR_MAPPED_ADDRESS],
    )
    .is_some()
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Required attributes not found in response",
        ));
    }
    if response.has_class(MessageClass::Success) {
        // presence checked by check_attribute_types() above
        let mapped_address = response
            .attribute::<XorMappedAddress>(XOR_MAPPED_ADDRESS)
            .unwrap();
        let visible_addr = mapped_address.addr(response.transaction_id());
        println!("found visible address {:?}", visible_addr);
        Ok(visible_addr)
    } else if response.has_class(MessageClass::Error) {
        println!("got error response {:?}", response);
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Error response",
        ))
    } else {
        println!("got unknown response {:?}", response);
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Unknown response",
        ))
    }
}


async fn udp_ice_get(socket: &UdpSocket, out: Message, to: SocketAddr) -> Result<SocketAddr, std::io::Error> {

    info!("generated to {}", out);
    let buf = out.to_bytes();
    trace!("generated to {:?}", buf);
    socket.send_to(&buf, to).await?;
    let mut buf = [0; 1500];
    let (amt, src) = socket.recv_from(&mut buf).await?;
    let buf = &buf[..amt];
    trace!("got {:?}", buf);
    let msg = Message::from_bytes(buf)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid message"))?;
    info!(
        "got from {:?} to {:?} {}",
        src,
        socket.local_addr().unwrap(),
        msg
    );

    parse_response(msg)
}


pub async fn get_socket_stun(socket: &UdpSocket)  -> Result<SocketAddr, std::io::Error>{
    let ice_server = SocketAddr::from_str("127.0.0.1:3478").unwrap();
    let mut msg = Message::new_request(BINDING);
    msg.add_fingerprint().unwrap();

    udp_ice_get(socket, msg, ice_server).await
}

#[allow(unused_assignments)]
pub async fn reader_and_writer(
    topic_gdp_name: GDPName,
    direction: String, // Sender or Receiver
    ros_tx : UnboundedSender<GDPPacket>,       // send to ros
    ebpf_tx: UnboundedSender<NewEbpfTopicRequest>,       // send to ebpf
    mut rtc_rx: UnboundedReceiver<GDPPacket>, // receive from ros
) {
    let mut need_more_data_for_previous_header = false;
    let mut remaining_gdp_header: GDPHeaderInTransit = GDPHeaderInTransit {
        action: GdpAction::Noop,
        destination: GDPName([0u8, 0, 0, 0]),
        source: GDPName([0u8, 0, 0, 0]),
        guid: GDPName([0u8, 0, 0, 0]),
        length: 0, // doesn't have any payload
    };
    let mut remaining_gdp_payload: Vec<u8> = vec![];
    let mut reset_counter = 0; // TODO: a temporary counter to reset the connection

    let stream = UdpSocket::bind("0.0.0.0:0").await.unwrap();
    let sock_public_addr = get_socket_stun(&stream).await;
    info!("UDP socket is bound to {:?}", sock_public_addr);

    let handle = tokio::spawn(register_stream(
        topic_gdp_name,
        direction,
        sock_public_addr.unwrap(),
        ebpf_tx
    ));

    loop {
        let mut receiving_buf = vec![0u8; UDP_BUFFER_SIZE];
        // Wait for the UDP socket to be readable
        // or new data to be sent
        tokio::select! {
            // _ = do_stuff_async()
            // async read is cancellation safe
            Ok((receiving_buf_size, _)) = stream.recv_from(&mut receiving_buf) => {
                // let receiving_buf_size = receiving_buf.len();
                let mut receiving_buf = receiving_buf[..receiving_buf_size].to_vec();
                info!("read {} bytes", receiving_buf_size);

                let mut header_payload_pair = vec!();

                // last time it has incomplete buffer to complete
                if need_more_data_for_previous_header {
                    let read_payload_size = remaining_gdp_payload.len() + receiving_buf_size;
                    if remaining_gdp_header.action == GdpAction::Noop {
                        warn!("last time it has incomplete buffer to complete, the action is Noop.");
                        // receiving_buf.append(&mut remaining_gdp_payload.clone());
                        remaining_gdp_payload.append(&mut receiving_buf[..receiving_buf_size].to_vec());
                        receiving_buf = remaining_gdp_payload.clone();
                        reset_counter += 1;
                        if reset_counter >5 {
                            error!("unable to match the buffer, reset the connection");
                            receiving_buf = vec!();
                            remaining_gdp_payload = vec!();
                            reset_counter = 0;
                        }
                    }
                    else if read_payload_size < remaining_gdp_header.length { //still need more things to read!
                        info!("more data to read. Current {}, need {}, expect {}", read_payload_size, remaining_gdp_header.length, remaining_gdp_header.length - read_payload_size);
                        remaining_gdp_payload.append(&mut receiving_buf[..receiving_buf_size].to_vec());
                        continue;
                    }
                    else if read_payload_size == remaining_gdp_header.length { // match the end of the packet
                        remaining_gdp_payload.append(&mut receiving_buf[..receiving_buf_size].to_vec());
                        header_payload_pair.push((remaining_gdp_header, remaining_gdp_payload.clone()));
                        receiving_buf = vec!();
                    }
                    else{ //overflow!!
                        // only get what's needed
                        warn!("The packet is overflowed!!! read_payload_size {}, remaining_gdp_header.length {}, remaining_gdp_payload.len() {}, receiving_buf_size {}", read_payload_size, remaining_gdp_header.length, remaining_gdp_payload.len(), receiving_buf_size);
                        let num_remaining = remaining_gdp_header.length - remaining_gdp_payload.len();
                        remaining_gdp_payload.append(&mut receiving_buf[..num_remaining].to_vec());
                        header_payload_pair.push((remaining_gdp_header, remaining_gdp_payload.clone()));
                        // info!("remaining_gdp_payload {:.unwrap()}", remaining_gdp_payload);

                        receiving_buf = receiving_buf[num_remaining..].to_vec();
                    }
                }

                let (mut processed_gdp_packets, processed_remaining_header) = parse_header_payload_pairs(receiving_buf.to_vec());
                header_payload_pair.append(&mut processed_gdp_packets);
                for (header, payload) in header_payload_pair {
                    let deserialized = header; //TODO: change the var name here

                    info!("the total received payload with size {:} with gdp header length {}",  payload.len(), header.length);

                    let packet = construct_gdp_packet_with_guid(deserialized.action, deserialized.destination, deserialized.source, payload, deserialized.guid);
                        match ros_tx.send(packet) {
                            Ok(_) => {},
                            Err(_) => {warn!("request is being handled by another connection");},
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
                        remaining_gdp_payload = vec!();
                    }
                }
            },

            Some(pkt_to_forward) = rtc_rx.recv() => {
                //info!("TCP packet to forward: {:.unwrap()}", pkt_to_forward);
                let transit_header = pkt_to_forward.get_header();
                let mut header_string = serde_json::to_string(&transit_header).unwrap();
                info!("the header size is {}", header_string.len());
                info!("the header to sent is {}", header_string);

                let destination_ip = get_non_existent_ip_addr(); 
                let destination = SocketAddr::new(std::net::IpAddr::V4(destination_ip), 8888);
                //insert the first null byte to separate the packet header
                header_string.push(0u8 as char);
                let header_string_payload = header_string.as_bytes();
                match stream.send_to(&header_string_payload[..header_string_payload.len()],destination).await {
                    Ok(_) => {},
                    Err(e) => {
                        warn!("The connection is closed: {}", e);
                        break;
                    }
                }

                // stream.write_all(&packet.payload[..packet.payload.len()]).await.unwrap();
                if let Some(payload) = pkt_to_forward.payload {
                    info!("the payload length is {}", payload.len());
                    stream.send_to(&payload[..payload.len()], destination).await.unwrap();
                }

                if let Some(name_record) = pkt_to_forward.name_record {
                    let name_record_string = serde_json::to_string(&name_record).unwrap();
                    let name_record_buffer = name_record_string.as_bytes();
                    info!("the name record length is {}", name_record_buffer.len());
                    stream.send_to(&name_record_buffer[..name_record_buffer.len()], destination).await.unwrap();
                }
            }

            else => {
                info!("The connection is closed");
                break;
            },
        }
    }

    futures::join!(handle);

    // loop {
    //     let n = dc.read(&mut buf).await.unwrap();
    //     println!("Read: \"{}\"", String::from_utf8_lossy(&buf[..n]));
    //     dc.write_all(b"Ping").await.unwrap();
    //     tokio::time::sleep(Duration::from_secs(2)).await;
    // }
}
