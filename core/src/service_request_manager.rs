use crate::{
    structs::{GDPName, GDPPacket, GdpAction}, connection_fib::{FibStateChange, FIBState, FibChangeAction, TopicStateInFIB},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use std::collections::HashSet;

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

    let mut response_forwarding_table: HashMap<GDPName, UnboundedSender<GDPPacket>> = HashMap::new();

    let mut processed_requests = HashSet::new();

    loop {
        tokio::select! {
            Some(pkt) = fib_rx.recv() => {

                match pkt.action {
                    GdpAction::Forward => {
                        warn!("forward????")
                    },
                    GdpAction::Request => {
                        info!("received GDP request {}", pkt);
                        warn!("request: {:?}", pkt.guid);
                        let topic_state = rib_state_table.get(&pkt.gdpname);
                        info!("the current topic state is {:?}", topic_state);
                        match topic_state {
                            Some(s) => {
                                if s.state == TopicStateInFIB::RUNNING {
                                    for dst in &s.receivers {
                                        let _ = dst.send(pkt.clone());
                                    }
                                } else {
                                    warn!("the current topic state is {:?}, not forwarded", topic_state)
                                }
                            },
                            None => {
                                error!("The gdpname {:?} does not exist", pkt.gdpname)
                            }
                        }
                    },
                    GdpAction::Response => {
                        info!("received GDP response {}", pkt);
                        warn!("response: {:?}", pkt.guid);
                        if processed_requests.contains(&pkt.guid) {
                            warn!("the request is processed, thrown away");
                            continue;
                        }else{
                            processed_requests.insert(pkt.guid);
                            // send it back with response forwarding table 
                            let dst = response_forwarding_table.get(&pkt.gdpname);
                            dst.unwrap().send(pkt.clone()).unwrap();
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
                                v.state = TopicStateInFIB::RUNNING;
                                v.receivers.push(update.forward_destination.unwrap());
                            }
                            None =>{
                                info!("Creating a new entry of gdp name {:?}", update.topic_gdp_name);
                                let state = FIBState {
                                    state: TopicStateInFIB::RUNNING,
                                    receivers: vec!(update.forward_destination.unwrap()),
                                };
                                rib_state_table.insert(
                                    update.topic_gdp_name,
                                    state,
                                );
                            }
                        };
                        // TODO: pause add
                    },
                    FibChangeAction::PAUSEADD => {
                        //todo pause
                    },
                    FibChangeAction::PAUSE => {
                        info!("Pausing GDP Name {:?}", update.topic_gdp_name);
                        match  rib_state_table.get_mut(&update.topic_gdp_name) {
                            Some(v) => {
                                v.state = TopicStateInFIB::PAUSED;
                            }
                            None =>{
                                error!("pausing non existing state!");
                            }
                        };
                    },
                    FibChangeAction::RESUME => {
                        info!("Deleting GDP Name {:?}", update.topic_gdp_name);
                        match  rib_state_table.get_mut(&update.topic_gdp_name) {
                            Some(v) => {
                                v.state = TopicStateInFIB::RUNNING;
                            }
                            None =>{
                                error!("resuming non existing state!");
                            }
                        };
                    },
                    FibChangeAction::DELETE => {
                        info!("Deleting GDP Name {:?}", update.topic_gdp_name);
                        // rib_state_table.remove(&update.topic_gdp_name);
                        match  rib_state_table.get_mut(&update.topic_gdp_name) {
                            Some(v) => {
                                v.state = TopicStateInFIB::DELETED;
                            }
                            None =>{
                                error!("deleting non existing state!");
                            }
                        };
                    },
                    FibChangeAction::RESPONSE => {
                        info!("Response channel received {:?}", update);
                        // insert the response channel
                        response_forwarding_table.insert(update.topic_gdp_name, update.forward_destination.unwrap());
                    }
                }
            }
        }
    }
}
