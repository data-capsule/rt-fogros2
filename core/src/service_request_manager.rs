use crate::structs::{GDPName, GDPPacket, GdpAction};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};


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
    REQUEST,
    RESPONSE,
    SENDER,
    RECEIVER,
    BIDIRECTIONAL,
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
    pub description: Option<String>,
}

#[derive(Debug)]
pub struct FibConnection {
    pub state: TopicStateInFIB,
    pub connection_type: FibConnectionType,
    tx: UnboundedSender<GDPPacket>,
    pub description: Option<String>,
}


#[derive(Debug)]
pub struct FIBState {
    pub receivers: Vec<FibConnection>,
}


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
                    GdpAction::Response => {
                        info!("received GDP response {}", pkt);
                        warn!("response: {:?}", pkt.guid);
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
                            // send it back with response forwarding table
                            // let dst = response_forwarding_table.get(&pkt.gdpname);
                            // match dst {
                            //     Some(v) => {
                            //         let _ = v.send(pkt.clone());
                            //     },
                            //     None => {
                            //         error!("the response channel does not exist");
                            //     }
                            // }
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
                                // v.state = TopicStateInFIB::RUNNING;
                                // v.receivers.push(update.forward_destination.unwrap());
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
                                // let state = FIBState {
                                //     state: TopicStateInFIB::RUNNING,
                                //     receivers: vec!(update.forward_destination.unwrap()),
                                // };
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
                        // TODO: pause add
                    },
                    FibChangeAction::STATE => {
                        
                    },
                    // FibChangeAction::RESPONSE => {
                    //     info!("Response channel received {:?}", update);
                    //     // insert the response channel
                    //     response_forwarding_table.insert(update.topic_gdp_name, update.forward_destination.unwrap());
                    // }
                    FibChangeAction::PAUSE => todo!(),
                    FibChangeAction::PAUSEADD => todo!(),
                    FibChangeAction::RESUME => todo!(),
                    FibChangeAction::DELETE => todo!(),
                    // FibChangeAction::PAUSEADD => {
                    //     //todo pause
                    // },
                    // FibChangeAction::PAUSE => {
                    //     info!("Pausing GDP Name {:?}", update.topic_gdp_name);
                    //     match  rib_state_table.get_mut(&update.topic_gdp_name) {
                    //         Some(v) => {
                    //             v.state = TopicStateInFIB::PAUSED;
                    //         }
                    //         None =>{
                    //             error!("pausing non existing state!");
                    //         }
                    //     };
                    // },
                    // FibChangeAction::RESUME => {
                    //     info!("Deleting GDP Name {:?}", update.topic_gdp_name);
                    //     match  rib_state_table.get_mut(&update.topic_gdp_name) {
                    //         Some(v) => {
                    //             v.state = TopicStateInFIB::RUNNING;
                    //         }
                    //         None =>{
                    //             error!("resuming non existing state!");
                    //         }
                    //     };
                    // },
                    // FibChangeAction::DELETE => {
                    //     info!("Deleting GDP Name {:?}", update.topic_gdp_name);
                    //     // rib_state_table.remove(&update.topic_gdp_name);
                    //     match  rib_state_table.get_mut(&update.topic_gdp_name) {
                    //         Some(v) => {
                    //             v.state = TopicStateInFIB::DELETED;
                    //         }
                    //         None =>{
                    //             error!("deleting non existing state!");
                    //         }
                    //     };
                    // },
                }
            }
        }
    }
}
