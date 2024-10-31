use crate::logger::{handle_logs, Logger};
use chrono::{self, format};
use fogrs_common::fib_structs::{
    FIBState, FibChangeAction, FibConnection, FibConnectionType, FibStateChange, TopicStateInFIB,
};
use fogrs_common::packet_structs::{GDPName, GDPPacket, GdpAction};
use std::collections::HashMap;
use std::collections::HashSet;
use std::time::SystemTime;
// for write_all()
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;

use default_net;

fn read_binding_table() -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {
    // get configuration file path from environment variable
    let config_file_path = std::env::var("SGC_CONFIG_PATH")?;
    let mut binding_table = HashMap::new();
    
    // read yaml 
    let config_file = std::fs::File::open(config_file_path)?;
    let config_file_reader = std::io::BufReader::new(config_file);
    let config: serde_yaml::Value = serde_yaml::from_reader(config_file_reader)?;
    let binding_table_yaml = config["bindings"].as_mapping() // get the binding table
        .ok_or("binding table not found")?;

    for (key, value) in binding_table_yaml {
        let key_str = key.as_str().ok_or("key is not a string")?;
        let value_str = value.as_str().ok_or("value is not a string")?;
        binding_table.insert(key_str.to_string(), value_str.to_string());
    }

    if binding_table.is_empty() {
        return Err("binding table is empty".into());
    }else {
        return Ok(binding_table);
    }
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

    let binding_table = match read_binding_table() {
        Ok(table) => Some(table),
        Err(e) => {
            warn!("Unable to read binding table: {:?}", e);
            None
        }
    };

    let mut rib_state_table: HashMap<GDPName, FIBState> = HashMap::new();

    let mut processed_requests = HashSet::new();

    let mut request_latency_table: HashMap<GDPName, SystemTime> = HashMap::new();

    let mut sent_requests_by_interface: HashMap<GDPName, Vec<String>> = HashMap::new();

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
                        if processed_requests.contains(&pkt.guid) {
                            warn!("the message {:?} is processed, thrown away", pkt.guid);
                            continue;
                        }
                        let topic_state = rib_state_table.get(&pkt.gdpname);
                        info!("the current topic state is {:?}", topic_state);
                        match topic_state {
                            Some(s) => {
                                for dst in &s.receivers {
                                    if dst.state == TopicStateInFIB::RUNNING && dst.connection_type == FibConnectionType::RECEIVER {
                                        info!("forwarding to {:?}", dst.description);
                                        processed_requests.insert(pkt.guid);
                                        match dst.tx.send(pkt.clone()) {
                                            Ok(_) => {
                                                // logger.log(format!("FORWARD, {:?}, {:?}", pkt.guid, pkt.source));
                                                info!("sent to {:?}", dst.description);
                                            },
                                            Err(e) => {
                                                // .expect(format!();
                                                warn!("failed to send to {:?}", dst.description);
                                            }
                                        }
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
                            logger.log(format!("REQUEST-DUP, {:?}, {:?}", pkt.guid.unwrap(), pkt.source));
                            warn!("the request is processed, thrown away");
                            continue;
                        }else{
                            // this is the first time receivng the request
                            request_latency_table.insert(pkt.gdpname, SystemTime::now());
                        }
                        let topic_state = rib_state_table.get(&pkt.gdpname);
                        info!("the current topic state is {:?}", topic_state);
                        let default_network_interface:String = match default_net::get_default_interface() {
                            Ok(default_interface) => {
                                default_interface.name
                            },
                            Err(e) => {
                                println!("{}", e);
                                continue;
                            },
                        };
                        match topic_state {
                            Some(s) => {
                                for dst in &s.receivers {
                                    if dst.state == TopicStateInFIB::RUNNING && dst.connection_type == FibConnectionType::REQUESTRECEIVER {

                                        if dst.interface.clone().unwrap() == "ros".to_string() {
                                            let _ = dst.tx.send(pkt.clone());
                                            println!("REQUEST, {:?}, {:?}, {:?}", pkt.guid.unwrap(), pkt.source, dst.interface.clone().unwrap());
                                            logger.log(format!("REQUEST, {:?}, {:?}, {:?}", pkt.guid.unwrap(), pkt.source, dst));
                                            continue;
                                        }

                                        else if dst.interface.clone().unwrap() == default_network_interface {
                                            let _ = dst.tx.send(pkt.clone());
                                            println!("REQUEST, {:?}, {:?}, {:?}", pkt.guid.unwrap(), pkt.source, dst.interface.clone().unwrap());
                                            logger.log(format!("REQUEST, {:?}, {:?}, {:?}", pkt.guid.unwrap(), pkt.source, dst));
                                            continue;
                                        }

                                        else {
                                            println!("the request to {:?} is not sent to the default interface {}, thrown away", dst.interface.clone(), default_network_interface);
                                            continue;
                                        }
                                        

                                        let interface = format!("{}-{}", dst.interface.clone().unwrap(), dst.address.clone().unwrap());
                                        if sent_requests_by_interface.contains_key(&pkt.guid.unwrap()) {
                                            let requests = sent_requests_by_interface.get_mut(&pkt.guid.unwrap()).unwrap();
                                            if !requests.contains(&interface) {
                                                requests.push(interface.clone());
                                                let _ = dst.tx.send(pkt.clone());
                                                println!("REQUEST, {:?}, {:?}, {:?}", pkt.guid.unwrap(), pkt.source, interface.clone());
                                                logger.log(format!("REQUEST, {:?}, {:?}, {:?}", pkt.guid.unwrap(), pkt.source, dst));
                                            }else{
                                                warn!("the request is already sent to interface {}, thrown away", interface);
                                                continue;
                                            }
                                        }else{
                                            sent_requests_by_interface.insert(pkt.guid.unwrap(), vec!(interface.clone()));
                                            let _ = dst.tx.send(pkt.clone());
                                            println!("REQUEST, {:?}, {:?}, {:?}", pkt.guid.unwrap(), pkt.source, interface.clone());
                                            logger.log(format!("REQUEST, {:?}, {:?}, {:?}", pkt.guid.unwrap(), pkt.source, dst));
                                        }

                                        
                                            
                                        // let _ = dst.tx.send(pkt.clone());
                                        // println!("REQUEST, {:?}, {:?}", pkt.guid.unwrap(), pkt.source);
                                        // logger.log(format!("REQUEST, {:?}, {:?}, {:?}", pkt.guid.unwrap(), pkt.source, dst));

                                        // let description = dst.description.clone().unwrap();
                                        // let description_parts: Vec<&str> = description.split(" ").collect();
                                        // let interface = description_parts[description_parts.len() - 1];
                                        // let ip_address = description_parts[description_parts.len() - 6].split(":").collect::<Vec<&str>>()[0];

                                        // // if (interface == "enp5s0" && ip_address == "54.177.240.220") || (interface == "wlo1" && ip_address == "54.176.160.12") { //two interfaces, two servers
                                        // if (interface == "wlo1" && ip_address == "54.177.240.220") || (interface == "wlo1" && ip_address == "54.176.160.12") { //one interface, two servers 
                                        // // if (interface == "enp5s0" && ip_address == "54.177.240.220") || (interface == "wlo1" && ip_address == "54.177.240.220") { //two interfaces, one server
                                        // // if (interface == "enp5s0" && ip_address == "54.177.240.220") { //one interface, one server
                                        // // if (interface == "wlo1" && ip_address == "54.176.160.12") { //one interface, one server
                                        //     // check if the request has been sent to this interface 
                                        //     if sent_requests_by_interface.contains_key(&pkt.guid.unwrap()) {
                                        //         let requests = sent_requests_by_interface.get_mut(&pkt.guid.unwrap()).unwrap();
                                        //         // if requests.contains(&interface.to_string()) {
                                        //         //     warn!("the request is already sent to this interface, thrown away");
                                        //         //     continue;
                                        //         // }else{
                                        //             requests.push(interface.to_string());
                                        //             let _ = dst.tx.send(pkt.clone());
                                        //             println!("REQUEST, {:?}, {:?}, {:?}, {:?}", pkt.guid.unwrap(), pkt.source, interface, ip_address);
                                        //             logger.log(format!("REQUEST, {:?}, {:?}", pkt.guid.unwrap(), pkt.source));
                                        //         // }
                                        //     }else{
                                        //         sent_requests_by_interface.insert(pkt.guid.unwrap(), vec!(interface.to_string()));
                                        //         let _ = dst.tx.send(pkt.clone());
                                        //         println!("REQUEST, {:?}, {:?}, {:?}, {:?}", pkt.guid.unwrap(), pkt.source, interface, ip_address);
                                        //         logger.log(format!("REQUEST, {:?}, {:?}", pkt.guid.unwrap(), pkt.source));
                                        //     }
                                        // }else{
                                        //     println!("not sent interface: {:?}, ip_address: {:?}", interface, ip_address);
                                        // }

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
                        
                        if processed_requests.contains(&pkt.guid) {
                            println!("RESPONSE-DUP, {:?}, {:?}, {}, {:?}", pkt.guid.unwrap(), pkt.source,  processing_time.as_micros(), pkt.description);
                            logger.log(format!("RESPONSE-DUP, {:?}, {:?}, {}, {:?}", pkt.guid.unwrap(), pkt.source,  processing_time.as_micros(), pkt.description));
                            warn!("the request is processed, thrown away");
                            continue;
                        }else{
                            println!("RESPONSE, {:?}, {:?}, {}, {:?}", pkt.guid.unwrap(), pkt.source,  processing_time.as_micros(), pkt.description);
                            logger.log(format!("RESPONSE, {:?}, {:?}, {}, {:?}", pkt.guid.unwrap(), pkt.source,  processing_time.as_micros(), pkt.description));
                            processed_requests.insert(pkt.guid);
                            let topic_state = rib_state_table.get(&pkt.gdpname);
                            info!("the current topic state is {:?}", topic_state);
                            match topic_state {
                                Some(s) => {
                                    for dst in &s.receivers {
                                        if dst.state == TopicStateInFIB::RUNNING && dst.connection_type == FibConnectionType::RESPONSERECEIVER {
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
                        if update.connection_type == FibConnectionType::RECEIVER || update.connection_type == FibConnectionType::REQUESTRECEIVER || update.connection_type == FibConnectionType::RESPONSERECEIVER {
                            
                            let mut in_binding_table = false;
                            let update_interface = update.interface.clone().unwrap();
                            let update_address = update.address.clone().unwrap(); // this is IP only address
                            // check the binding table; if the table has matching entry, then set should_filter to true
                            if let Some(table) = &binding_table {
                                if let Some(address) = table.get(&update_interface) {
                                    if update_address.starts_with(address) {
                                        info!("interface {:?} is added to FIB", update);
                                        in_binding_table = true;
                                    }
                                }
                            }
                            if binding_table.is_some() && update_interface != "ros".to_string() && !in_binding_table {
                                warn!("interface {} is not added to FIB, because it is not in the binding table {:?}", update_interface, binding_table);
                                continue;
                            }
                            
                            match  rib_state_table.get_mut(&update.topic_gdp_name) {
                                Some(v) => {
                                        info!("local topic interface {:?} is added", update);
                                        // check binding table 
                                        v.receivers.push(FibConnection{
                                            state: TopicStateInFIB::RUNNING,
                                            connection_type: update.connection_type,
                                            tx: update.forward_destination.unwrap(),
                                            description: update.description,
                                            interface: update.interface,
                                            address: update.address,
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
                                            interface: update.interface,
                                            address: update.address,
                                        }),
                                    };
                                    rib_state_table.insert(
                                        update.topic_gdp_name,
                                        state,
                                    );
                                }
                            }
                        }else{
                            warn!("sender connection type {:?}, not added to FIB", update.connection_type);
                            continue;
                        }
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
