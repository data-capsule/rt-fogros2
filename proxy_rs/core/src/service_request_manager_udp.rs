use crate::logger::{handle_logs, Logger};
use chrono;
use fogrs_common::fib_structs::{
    FIBState, FibChangeAction, FibConnection, FibConnectionType, FibStateChange, TopicStateInFIB,
};
use fogrs_common::packet_structs::{GDPName, GDPPacket, GdpAction};
use std::collections::HashMap;
use std::collections::HashSet;
use std::time::SystemTime;
// for write_all()
use tokio::sync::mpsc;
use tokio::sync::mpsc::{UnboundedReceiver};

/// receive, check, and route GDP messages
///
/// receive from a pool of receiver connections (one per interface)
/// use a hash table to figure out the corresponding
///     hash table <gdp_name, send_tx>
///     TODO: use future if the destination is unknown
/// forward the packet to corresponding send_tx
pub async fn service_connection_fib_handler(
    mut fib_rx: UnboundedReceiver<GDPPacket>, // its tx used to transmit data to fib
    // mut response_rx: UnboundedReceiver<GDPPacket>, // its tx used to transmit data to fib
    mut channel_rx: UnboundedReceiver<FibStateChange>, /* its tx used to update fib with new names/records */
) {
    let mut rib_state_table: HashMap<GDPName, FIBState> = HashMap::new();

    let mut processed_requests = HashSet::new();

    let mut request_latency_table: HashMap<GDPName, SystemTime> = HashMap::new();

    let (tx, rx) = mpsc::unbounded_channel();
    let logger = Logger::new(tx);

    let file_name = format!(
        "/tmp/latency-{}.log",
        chrono::offset::Local::now().timestamp()
    );
    println!("Logger file name is {}", file_name);
    let log_handler = tokio::spawn(handle_logs(rx, file_name));

    loop {
        tokio::select! {
            Some(pkt) = fib_rx.recv() => {

                match pkt.action {
                    GdpAction::Forward => {
                        let topic_state = rib_state_table.get(&pkt.gdpname);
                        info!("the current topic state is {:?}", topic_state);
                        match topic_state {
                            Some(s) => {
                                for dst in &s.receivers {
                                    if dst.state == TopicStateInFIB::RUNNING && dst.connection_type == FibConnectionType::RECEIVER {
                                        let _ = dst.tx.send(pkt.clone());
                                    } else {
                                        warn!("the current topic {:?} with {:?}, not forwarded", dst.connection_type, dst.description);
                                    }
                                }
                            },
                            None => {
                                error!("The gdpname {:?} does not exist", pkt.gdpname)
                            }
                        }
                    },
                    GdpAction::Request => {
                        info!("received GDP request {}", pkt);
                        warn!("request: {:?}", pkt.guid);
                        if processed_requests.contains(&pkt.guid) {
                            warn!("the request is processed, thrown away");
                            continue;
                        }
                        let topic_state = rib_state_table.get(&pkt.gdpname);
                        info!("the current topic state is {:?}", topic_state);
                        match topic_state {
                            Some(s) => {
                                for dst in &s.receivers {
                                    if dst.state == TopicStateInFIB::RUNNING && dst.connection_type == FibConnectionType::REQUEST {
                                        request_latency_table.insert(pkt.gdpname, SystemTime::now());
                                        let _ = dst.tx.send(pkt.clone());
                                        logger.log(format!("REQUEST, {:?}, {:?}", pkt.guid.unwrap(), pkt.source));
                                    } else {
                                        warn!("the current topic {:?} with {:?}, not forwarded", dst.connection_type, dst.description);
                                    }
                                }
                            },
                            None => {
                                error!("The gdpname {:?} does not exist", pkt.gdpname)
                            }
                        }
                    },
                    GdpAction::Response => {
                        info!("received GDP response {:?}", pkt);
                        warn!("response: {:?} from {:?}", pkt.guid, pkt.source);
                        let processing_time = SystemTime::now().duration_since(request_latency_table.get(&pkt.gdpname).unwrap().clone()).unwrap();
                        logger.log(format!("RESPONSE, {:?}, {:?}, {}", pkt.guid.unwrap(), pkt.source,  processing_time.as_micros()));
                        if processed_requests.contains(&pkt.guid) {
                            warn!("the request is processed, thrown away");
                            continue;
                        }else{
                            processed_requests.insert(pkt.guid);
                            let topic_state = rib_state_table.get(&pkt.gdpname);
                            info!("the current topic state is {:?}", topic_state);
                            match topic_state {
                                Some(s) => {
                                    for dst in &s.receivers {
                                        if dst.state == TopicStateInFIB::RUNNING && dst.connection_type == FibConnectionType::RESPONSE {
                                            let _ = dst.tx.send(pkt.clone());
                                        } else {
                                            warn!("the current topic {:?} with {:?}, not forwarded", dst.connection_type, dst.description);
                                        }
                                    }
                                },
                                None => {
                                    error!("The gdpname {:?} does not exist", pkt.gdpname)
                                }
                            }
                        }
                    },

                    _ => {
                        error!("Unknown action {:?}", pkt.action);
                        continue;
                    }
                }

            }

            // update the table
            Some(update) = channel_rx.recv() => {
                match update.action {
                    FibChangeAction::ADD => {
                        info!("update status received {:?}", update);
                        match  rib_state_table.get_mut(&update.topic_gdp_name) {
                            Some(v) => {
                                info!("local topic interface {:?} is added", update);
                                v.receivers.push(FibConnection{
                                    state: TopicStateInFIB::RUNNING,
                                    connection_type: update.connection_type,
                                    tx: update.forward_destination.unwrap(),
                                    description: update.description,
                                });
                            }
                            None =>{
                                info!("Creating a new entry of {:?}", update);
                                let state = FIBState {
                                    receivers: vec!(FibConnection{
                                        state: TopicStateInFIB::RUNNING,
                                        connection_type: update.connection_type,
                                        tx: update.forward_destination.unwrap(),
                                        description: update.description,
                                    }),
                                };
                                rib_state_table.insert(
                                    update.topic_gdp_name,
                                    state,
                                );
                            }
                        };
                    },
                    _ => {
                        error!("Unknown action {:?}", update.action);
                        continue;
                    }
                }
            }
        }
    }
}
