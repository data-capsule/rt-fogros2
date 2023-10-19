use crate::api_server::ROSTopicRequest;
use crate::connection_fib::connection_fib_handler;

#[cfg(feature = "ros")]
use crate::network::webrtc::{register_webrtc_stream, webrtc_reader_and_writer};

use crate::pipeline::{construct_gdp_forward_from_bytes, construct_gdp_request_with_guid, construct_gdp_response_with_guid};
use crate::service_request_manager::{service_connection_fib_handler, FibConnectionType};
use crate::structs::{
    gdp_name_to_string, generate_random_gdp_name, get_gdp_name_from_topic, GDPName, GdpAction,
    Packet, generate_gdp_name_from_string, GDPPacket,
};

use crate::service_request_manager::{FibChangeAction, FibStateChange};
use hyper::client::connect::Connect;
use serde::{Deserialize, Serialize};

use std::env;
use std::sync::{Arc, Mutex};
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use std::str;


use crate::db::*;
use futures::{StreamExt};
use redis_async::{client, resp::FromResp};

use tokio::sync::mpsc::{self};

use tokio::time::Duration;
use byteorder::{ByteOrder, LittleEndian};


#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize, Hash)]
pub enum TopicManagerAction {
    ADD,
    PAUSE, // pausing the forwarding of the topic, keeping connections alive
    PAUSEADD, // adding the entry to FIB, but keeps it paused
    RESUME, // resume a paused topic
    DELETE, // deleting a local topic interface and all its connections 
    RESPONSE,
}


pub struct TopicManagerRequest {
    action: TopicManagerAction,
    topic_name: String,
    topic_type: String,
    certificate: Vec<u8>,
}


// pub struct rmw_request_id_s {
// pub writer_guid: [i8; 16usize],
// pub sequence_number: i64,
// }
// generate [u8:4] from the above struct 

// pub fn get_service_guid_from_request(
//     request: r2r::UntypedServiceRequest
// ) -> GDPName {
//     // GDPANME = [u8:4]
//     let mut service_guid = GDPName([0; 4]);
//     let mut writer_guid = [0; 16];
//     let mut sequence_number = 0;
//     let mut request_id = request.request_id;
//     // let mut request_id_bytes = request_id.as_bytes();
//     let mut writer_guid_bytes = request_id.writer_guid;//&request_id_bytes[0..16];
//     let mut sequence_number_bytes = request_id.sequence_number;//&request_id_bytes[16..24];
//     writer_guid.copy_from_slice(&writer_guid_bytes);
//     sequence_number = i64::from_le_bytes(sequence_number_bytes.try_into().unwrap());
//     service_guid.0.copy_from_slice(&writer_guid[0..4]);
//     service_guid   
// }


// ROS service(provider) -> webrtc (publish remotely); webrtc -> local service client
pub async fn ros_topic_remote_service_provider(
    mut status_recv: UnboundedReceiver<TopicManagerRequest>,
    mut fib_tx : UnboundedSender<GDPPacket>,
    mut channel_tx: UnboundedSender<FibStateChange>,
) {
    let mut join_handles = vec![];

    // let (request_tx, request_rx) = mpsc::unbounded_channel();
    // let (response_tx, response_rx) = mpsc::unbounded_channel();
    // let (channel_tx, channel_rx) = mpsc::unbounded_channel();
    // let fib_handle: tokio::task::JoinHandle<()> = tokio::spawn(async move {
    //     service_connection_fib_handler(request_rx, response_rx, channel_rx).await;
    // });
    // join_handles.push(fib_handle);


    let ctx = r2r::Context::create().expect("context creation failure");
    let node = Arc::new(Mutex::new(
        r2r::Node::create(ctx, "sgc_remote_service", "namespace").expect("node creation failure"),
    ));

    let ros_manager_node_clone = node.clone();
    let _handle = tokio::task::spawn_blocking(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        ros_manager_node_clone
            .clone()
            .lock()
            .unwrap()
            .spin_once(std::time::Duration::from_millis(10));
    });

    let mut existing_topics = vec!();

    loop {
        tokio::select! {
            Some(request) = status_recv.recv() => {
                let topic_name = request.topic_name;
                let topic_type = request.topic_type;
                let action = request.action;
                let certificate = request.certificate;
                let request_tx = fib_tx.clone();
                let topic_gdp_name = GDPName(get_gdp_name_from_topic(
                    &topic_name,
                    &topic_type,
                    &certificate,
                ));

                if request.action != TopicManagerAction::ADD {
                    // let channel_update_msg = FibStateChange {
                    //     action: request.action,
                    //     topic_gdp_name: topic_gdp_name,
                    //     forward_destination: None,
                    // };
                    // let _ = channel_tx.send(channel_update_msg);
                    error!("action {:?} not supported in ros_topic_remote_service_provider", request.action);
                    continue;
                }

                let manager_node = node.clone();

                info!(
                    "[ros_topic_remote_publisher_handler] topic creator for topic {}, type {}, action {:?}",
                    topic_name, topic_type, action
                );

                // ROS subscriber -> FIB -> RTC
                let (ros_tx, mut ros_rx) = mpsc::unbounded_channel();

                if existing_topics.contains(&topic_gdp_name) {
                    info!("topic {:?} already exists in existing topics; don't need to create another subscriber", topic_gdp_name);
                } else {
                    
                    let channel_update_msg = FibStateChange {
                        action: FibChangeAction::ADD,
                        connection_type: FibConnectionType::RESPONSE,
                        topic_gdp_name: topic_gdp_name,
                        forward_destination: Some(ros_tx),
                        description: Some("ros service response".to_string()),
                    };
                    let _ = channel_tx.send(channel_update_msg);

                    existing_topics.push(topic_gdp_name);
                    let mut service = manager_node.lock().unwrap()
                    .create_service_untyped(&topic_name, &topic_type)
                    .expect("topic subscribing failure");

                    // let ros_handle = tokio::spawn(async move {
                    //     info!("ROS handling loop has started!");
                    //     while let Some(packet) = subscriber.next().await {
                    //         // info!("received a ROS packet {:?}", packet);
                    //         let ros_msg = packet;
                    //         let packet = construct_gdp_forward_from_bytes(topic_gdp_name, topic_gdp_name, ros_msg );
                    //         fib_tx.send(packet).expect("send for ros subscriber failure");
                    //     }
                    // });

                    let ros_handle = tokio::spawn (async move {
                        loop {
                            tokio::select!{
                                Some(req) = service.next() => {
                                    // send it to webrtc 
                                    //packet guid is the hash of the request id as string 
                                    let guid = format!("{:?}", req.request_id);
                                    info!("received a ROS request {:?}", guid);
                                    let packet_guid = generate_gdp_name_from_string(&guid); 
                                    let packet = construct_gdp_request_with_guid(topic_gdp_name, topic_gdp_name, serde_json::to_vec(&req.message).unwrap(), packet_guid );
                                    info!("sending to webrtc {:?}", packet);
                                    request_tx.send(packet).expect("send for ros subscriber failure");
                                    tokio::select! {
                                        Some(packet) = ros_rx.recv() => {
                                            // send it to ros
                                            // let msg = r2r::std_msgs::String::from_bytes(&packet).unwrap();
                                            // service.send_response(msg).await.expect("send for ros subscriber failure");
                                            
                                            info!("received from webrtc in ros_rx {:?}", packet);
                                            let mut respond_msg = (r2r::UntypedServiceSupport::new_from(&topic_type).unwrap().make_response_msg)();
                                            let respond_msg_in_json = serde_json::from_str(str::from_utf8(&packet.payload.unwrap()).unwrap()).expect("json parsing failure");
                                            respond_msg.from_json(respond_msg_in_json).unwrap();
                                            req.respond(respond_msg).expect("could not send service response");
                                        }, 
                                        // timeout after 1 second 
                                        _ = tokio::time::sleep(Duration::from_millis(1000)) => {
                                            error!("timeout for ros_rx");
                                            let mut respond_msg = (r2r::UntypedServiceSupport::new_from(&topic_type).unwrap().make_response_msg)();
                                            req.respond(respond_msg).expect("could not send service response");
                                        }
                                    }
                                }, 
                            }
                        }
                    }
                    );



                    join_handles.push(ros_handle);
                }   
            }
        }
    }
}

// webrtc -> sgc_local_service_caller (call the service locally) -> webrtc
pub async fn ros_topic_local_service_caller(
    mut status_recv: UnboundedReceiver<TopicManagerRequest>,
    mut fib_tx : UnboundedSender<GDPPacket>,
    mut channel_tx: UnboundedSender<FibStateChange>,
) {
    info!("ros_topic_remote_subscriber_handler has started");
    let mut join_handles = vec![];

    let ctx = r2r::Context::create().expect("context creation failure");
    let node = Arc::new(Mutex::new(
        r2r::Node::create(ctx, "sgc_local_service_caller", "namespace")
            .expect("node creation failure"),
    ));

    let ros_manager_node_clone = node.clone();
    let _handle = tokio::task::spawn_blocking(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        ros_manager_node_clone
            .clone()
            .lock()
            .unwrap()
            .spin_once(std::time::Duration::from_millis(10));
    });


    let mut existing_topics = vec!();

    loop {
        tokio::select! {
            Some(request) = status_recv.recv() => {
                let topic_name = request.topic_name;
                let topic_type = request.topic_type;
                let action = request.action;
                let certificate = request.certificate;
                let fib_tx = fib_tx.clone();
                let topic_gdp_name = GDPName(get_gdp_name_from_topic(
                    &topic_name,
                    &topic_type,
                    &certificate,
                ));

                if request.action != TopicManagerAction::ADD {
                    // let channel_update_msg = FibStateChange {
                    //     action: request.action,
                    //     topic_gdp_name: topic_gdp_name,
                    //     forward_destination: None,
                    // };
                    // let _ = channel_tx.send(channel_update_msg);
                    error!("action {:?} not supported in ros_topic_remote_service_provider", request.action);
                    continue;
                }

                // let stream = request.stream.unwrap();
                let manager_node = node.clone();


                info!(
                    "[local_service_caller] topic creator for topic {}, type {}, action {:?}",
                    topic_name, topic_type, action
                );


                // RTC -> FIB -> ROS publisher
                let (ros_tx, mut ros_rx) = mpsc::unbounded_channel();

                let channel_update_msg = FibStateChange {
                    action: FibChangeAction::ADD,
                    connection_type: FibConnectionType::REQUEST,
                    topic_gdp_name: topic_gdp_name,
                    forward_destination: Some(ros_tx),
                    description: Some("ros service request".to_string()),
                };
                let _ = channel_tx.send(channel_update_msg);


                // let rtc_handle = tokio::spawn(webrtc_reader_and_writer(stream, fib_tx.clone(), rtc_rx));
                // join_handles.push(rtc_handle);

                if existing_topics.contains(&topic_gdp_name) {
                    info!("topic {:?} already exists in existing topics; don't need to create another publisher", topic_gdp_name);
                } else {
                    existing_topics.push(topic_gdp_name);
                    let untyped_client = manager_node.lock().unwrap()
                    .create_client_untyped(&topic_name, &topic_type)
                    .expect("topic publisher create failure");
                    
                    // receive from the rtc_rx and call the local service 
                    let ros_handle = tokio::spawn(async move {
                        info!("[ros_topic_local_service_caller] ROS handling loop has started!");
                        loop{
                            let pkt_to_forward = ros_rx.recv().await.unwrap();
                            if pkt_to_forward.action == GdpAction::Request {
                                info!("new payload to publish {:?}", pkt_to_forward.guid);
                                if pkt_to_forward.gdpname == topic_gdp_name {
                                    let payload = pkt_to_forward.get_byte_payload().unwrap();
                                    let ros_msg = serde_json::from_str(str::from_utf8(payload).unwrap()).expect("json parsing failure");
                                    info!("the decoded payload to publish is {:?}", ros_msg);
                                    let mut resp = untyped_client.request(ros_msg).expect("service call failure").await;
                                    info!("the response is {:?}", resp);
                                    //send back the response to the rtc
                                    let packet_guid = pkt_to_forward.guid.unwrap(); //generate_gdp_name_from_string(&stringify!(resp.request_id)); 
                                    let packet = construct_gdp_response_with_guid(topic_gdp_name, topic_gdp_name, serde_json::to_vec(&resp.unwrap().unwrap()).unwrap(), packet_guid);
                                    fib_tx.send(packet).expect("send for ros subscriber failure");
                                } else{
                                    warn!("{:?} received a packet for name {:?}",pkt_to_forward.gdpname, topic_gdp_name);
                                }
                            } else {
                                warn!("received a packet with action {:?} in ros_topic_local_service_caller", pkt_to_forward.action);
                            }
                        }
                    });

                   join_handles.push(ros_handle);


                }
                
            }
        }
    }
}


#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
pub struct RosTopicStatus {
    pub action: String,
}

pub async fn ros_service_manager(mut service_request_rx: UnboundedReceiver<ROSTopicRequest>) {
    let mut waiting_rib_handles = vec![];

    // TODO: now it's hardcoded, make it changable later
    let crypto_name = "test_cert";
    let crypto_path = match env::var_os("SGC_CRYPTO_PATH") {
        Some(config_file) => {
            config_file.into_string().unwrap()
        },
        None => format!(
            "./sgc_launch/configs/crypto/{}/{}-private.pem",
            crypto_name, crypto_name
        ),
    };
    
    let certificate = std::fs::read(crypto_path)
    .expect("crypto file not found!");

    let (fib_tx, fib_rx) = mpsc::unbounded_channel();
    let (channel_tx, channel_rx) = mpsc::unbounded_channel();
    let fib_handle = tokio::spawn(async move {
        service_connection_fib_handler(fib_rx, channel_rx).await;
    });
    waiting_rib_handles.push(fib_handle);

    let (publisher_operation_tx, publisher_operation_rx) = mpsc::unbounded_channel();
    let channel_tx_clone = channel_tx.clone();
    let fib_tx_clone = fib_tx.clone();
    let topic_creator_handle = tokio::spawn(async move {
        ros_topic_remote_service_provider(publisher_operation_rx, fib_tx_clone, channel_tx_clone).await;
    });
    waiting_rib_handles.push(topic_creator_handle);

    let (subscriber_operation_tx, subscriber_operation_rx) = mpsc::unbounded_channel();
    let channel_tx_clone = channel_tx.clone();
    let fib_tx_clone = fib_tx.clone();
    let topic_creator_handle = tokio::spawn(async move {
        // This is because the ROS node creation is not thread safe 
        // See: https://github.com/ros2/rosbag2/issues/329
        std::thread::sleep(std::time::Duration::from_millis(500));
        ros_topic_local_service_caller(subscriber_operation_rx,  fib_tx_clone, channel_tx_clone).await;
    });
    waiting_rib_handles.push(topic_creator_handle);

    loop {
        select! {
            Some(payload) = service_request_rx.recv() => {
                // info!("ros topic manager get payload: {:?}", payload);
                match payload.api_op.as_str() {
                    "add" => {
                        let topic_name = payload.topic_name;
                        let topic_type = payload.topic_type;
                        let action = payload.ros_op;
                        let topic_gdp_name = GDPName(get_gdp_name_from_topic(
                            &topic_name,
                            &topic_type,
                            &certificate,
                        ));
                        info!("detected a new topic {:?} with action {:?}, topic gdpname {:?}", topic_name, action, topic_gdp_name);
                        match action.as_str() {
                            // provide service locally and send to remote service
                            "client" => { 
                                let topic_name_clone = topic_name.clone();
                                let certificate = certificate.clone();
                                let topic_operation_tx = publisher_operation_tx.clone();
                                // let handle = tokio::spawn(
                                //     async move {
                                //         create_new_local_service_caller(topic_gdp_name, topic_name_cloned, topic_type, certificate,
                                //             topic_operation_tx).await;
                                //     }
                                // );
                                // waiting_rib_handles.push(handle);
                                let topic_creator_request = TopicManagerRequest {
                                    action: TopicManagerAction::ADD,
                                    topic_name: topic_name_clone,
                                    topic_type: topic_type,
                                    certificate: certificate,
                                };
                                let _ = topic_operation_tx.send(topic_creator_request);
                            }

                            // provide service remotely and interact with local service, and send back
                            "service" => {
                                let topic_name_clone = topic_name.clone();
                                let certificate = certificate.clone();
                                let topic_type = topic_type.clone();
                                let topic_operation_tx = subscriber_operation_tx.clone();
                                let topic_creator_request = TopicManagerRequest {
                                    action: TopicManagerAction::ADD,
                                    topic_name: topic_name_clone,
                                    topic_type: topic_type,
                                    certificate: certificate,
                                };
                                let _ = topic_operation_tx.send(topic_creator_request);
                            }, 
                            _ => {
                                warn!("unknown action {}", action);
                            }
                        };
                    },
                    "routing" => {
                        let topic_name = payload.topic_name;
                        let topic_type = payload.topic_type;
                        let action = payload.ros_op;
                        let topic_gdp_name = GDPName(get_gdp_name_from_topic(
                                &topic_name,
                                &topic_type,
                                &certificate,
                            ));
                            
                        match action.as_str() {

                            // provide service locally and send to remote service
                            "client" => { 
                                let topic_name_clone = topic_name.clone();
                                let certificate = certificate.clone();
                                let topic_operation_tx = publisher_operation_tx.clone();
                                // let handle = tokio::spawn(
                                //     async move {
                                //         create_new_local_service_caller(topic_gdp_name, topic_name_cloned, topic_type, certificate,
                                //             topic_operation_tx).await;
                                //     }
                                // );
                                // waiting_rib_handles.push(handle);
                                let topic_creator_request = TopicManagerRequest {
                                    action: TopicManagerAction::ADD,
                                    topic_name: topic_name_clone,
                                    topic_type: topic_type,
                                    certificate: certificate,
                                };
                                let _ = topic_operation_tx.send(topic_creator_request);
                            }

                            // provide service remotely and interact with local service, and send back
                            "service" => {
                                let topic_name_clone = topic_name.clone();
                                let certificate = certificate.clone();
                                let topic_type = topic_type.clone();
                                let topic_operation_tx = subscriber_operation_tx.clone();
                                let topic_creator_request = TopicManagerRequest {
                                    action: TopicManagerAction::ADD,
                                    topic_name: topic_name_clone,
                                    topic_type: topic_type,
                                    certificate: certificate,
                                };
                                let _ = topic_operation_tx.send(topic_creator_request);
                            }

                            // source: fib -> webrtc 
                            "source" => {
                                let (local_to_rtc_tx, local_to_rtc_rx) = mpsc::unbounded_channel();
                                // let sender_url = "sender".to_string();
                                let sender_url = payload.forward_sender_url.unwrap();
                                let webrtc_stream = register_webrtc_stream(&sender_url, None).await;
                                let fib_tx_clone = fib_tx.clone();
                                let rtc_handle = tokio::spawn(webrtc_reader_and_writer(webrtc_stream, fib_tx_clone, local_to_rtc_rx));
                                waiting_rib_handles.push(rtc_handle);
                                // let channel_update_msg = FibStateChange {
                                //     action: FibChangeAction::ADD,
                                //     topic_gdp_name: topic_gdp_name,
                                //     forward_destination: Some(local_to_rtc_tx),
                                // };
                                // let _ = channel_tx.send(channel_update_msg);
                            }

                            // destination: webrtc -> fib
                            "destination" => {
                                let (local_to_rtc_tx, local_to_rtc_rx) = mpsc::unbounded_channel();
                                let fib_tx_clone = fib_tx.clone();
                                // let receiver_url = "receiver".to_string();
                                // let peer_dialing_url = "sender".to_string();
                                let receiver_url = payload.forward_receiver_url.unwrap();
                                let peer_dialing_url = payload.forward_sender_url.unwrap();
                                let webrtc_stream =
                                    register_webrtc_stream(&receiver_url, Some(peer_dialing_url)).await;
                                let rtc_handle = tokio::spawn(webrtc_reader_and_writer(webrtc_stream, fib_tx_clone, local_to_rtc_rx));
                                waiting_rib_handles.push(rtc_handle);
                                let channel_update_msg = FibStateChange {
                                    action: FibChangeAction::ADD,
                                    topic_gdp_name: topic_gdp_name,
                                    connection_type: FibConnectionType::REQUEST, //TODO
                                    forward_destination: Some(local_to_rtc_tx),
                                    description: Some("webrtc stream".to_string()),
                                };
                                let _ = channel_tx.send(channel_update_msg);
                            }
                            _ => {
                                warn!("unknown action {}", action);
                            }
                        }
                    }
                    // "del" => {
                    //     info!("deleting topic {:?}", payload);
                        
                    //     match payload.ros_op.as_str() {
                    //         "pub" => {
                    //             let topic_operation_tx = publisher_operation_tx.clone();
                    //             let topic_creator_request = TopicManagerRequest {
                    //                 action: TopicManagerAction::DELETE,
                    //                 topic_name: payload.topic_name,
                    //                 topic_type: payload.topic_type,
                    //                 certificate: certificate.clone(),
                    //             };
                    //             let _ = topic_operation_tx.send(topic_creator_request);

                    //         }, 
                    //         "sub" => {
                    //             let topic_operation_tx = subscriber_operation_tx.clone();
                    //             let topic_creator_request = TopicManagerRequest {
                    //                 action: TopicManagerAction::DELETE,
                    //                 topic_name: payload.topic_name,
                    //                 topic_type: payload.topic_type,
                    //                 certificate: certificate.clone(),
                    //             };
                    //             let _ = topic_operation_tx.send(topic_creator_request);
                    //         }
                    //         _ => {
                    //             warn!("unknown action {}", payload.ros_op);
                    //         }
                    //     }
                    // },               
                    // "resume" => {
                    //     info!("resuming topic {:?}", payload);
                        
                    //     match payload.ros_op.as_str() {
                    //         "pub" => {
                    //             let topic_operation_tx = publisher_operation_tx.clone();
                    //             let topic_creator_request = TopicManagerRequest {
                    //                 action: TopicManagerAction::RESUME,
                    //                 topic_name: payload.topic_name,
                    //                 topic_type: payload.topic_type,
                    //                 certificate: certificate.clone(),
                    //             };
                    //             let _ = topic_operation_tx.send(topic_creator_request);

                    //         }, 
                    //         "sub" => {
                    //             let topic_operation_tx = subscriber_operation_tx.clone();
                    //             let topic_creator_request = TopicManagerRequest {
                    //                 action: TopicManagerAction::RESUME,
                    //                 topic_name: payload.topic_name,
                    //                 topic_type: payload.topic_type,
                    //                 certificate: certificate.clone(),
                    //             };
                    //             let _ = topic_operation_tx.send(topic_creator_request);
                    //         }
                    //         _ => {
                    //             warn!("unknown action {}", payload.ros_op);
                    //         }
                    //     }
                    // }

                    _ => {
                        info!("operation {} not handled!", payload.api_op);
                    }
                }
            },
        }
    }
}
