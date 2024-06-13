use fogrs_common::packet_structs::Packet;
use fogrs_common::{
    fib_structs::{FibChangeAction, FibConnectionType, FibStateChange},
    packet_structs::{
        construct_gdp_forward_from_bytes, generate_random_gdp_name,
        get_gdp_name_from_topic, GDPName, GDPPacket, GdpAction,
    },
};
use futures::StreamExt;
use log::{error, info};
use r2r::Node;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::{
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task,
    time::Duration,
};

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize, Hash)]
pub enum TopicManagerAction {
    ADD,
    PAUSE,    // pausing the forwarding of the topic, keeping connections alive
    PAUSEADD, // adding the entry to FIB, but keeps it paused
    RESUME,   // resume a paused topic
    DELETE,   // deleting a local topic interface and all its connections
    RESPONSE,
}

pub struct TopicManagerRequest {
    pub action: TopicManagerAction,
    pub topic_name: String,
    pub topic_type: String,
    pub certificate: Vec<u8>,
}

#[derive(Clone)]
pub struct ROSManager {
    node: Arc<Mutex<r2r::Node>>,
    unique_ros_node_gdp_name: GDPName,
}

impl ROSManager {
    pub fn new(node_name: &str, namespace: &str) -> Self {
        let ctx = r2r::Context::create().expect("context creation failure");
        let node = Arc::new(Mutex::new(
            Node::create(ctx, node_name, namespace).expect("node creation failure"),
        ));
        let unique_ros_node_gdp_name = generate_random_gdp_name();

        let ros_manager_node_clone = node.clone();
        task::spawn_blocking(move || loop {
            std::thread::sleep(Duration::from_millis(100));
            ros_manager_node_clone
                .lock()
                .unwrap()
                .spin_once(Duration::from_millis(10));
        });

        Self {
            node,
            unique_ros_node_gdp_name,
        }
    }

    // local ROS topic(provider) -> fib
    pub async fn handle_ros_topic_remote_publisher(
        self, mut status_recv: UnboundedReceiver<TopicManagerRequest>,
        fib_tx: UnboundedSender<GDPPacket>, channel_tx: UnboundedSender<FibStateChange>,
    ) {
        let mut join_handles = vec![];

        let ros_manager_node_clone = self.node.clone();
        let _handle = tokio::task::spawn_blocking(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
            ros_manager_node_clone
                .clone()
                .lock()
                .unwrap()
                .spin_once(std::time::Duration::from_millis(10));
        });

        let mut existing_topics = vec![];

        loop {
            tokio::select! {
                Some(request) = status_recv.recv() => {
                    let topic_name = request.topic_name;
                    let topic_type = request.topic_type;
                    let action = request.action;
                    let certificate = request.certificate;
                    let topic_gdp_name = GDPName(get_gdp_name_from_topic(
                        &topic_name,
                        &topic_type,
                        &certificate,
                    ));

                    if request.action != TopicManagerAction::ADD {
                        error!("action {:?} not supported in ros_remote_service_provider", request.action);
                        continue;
                    }

                    let manager_node = self.node.clone();

                    info!(
                        "[ros_topic_remote_publisher_handler] topic creator for topic {}, type {}, action {:?}",
                        topic_name, topic_type, action
                    );

                    // ROS subscriber -> FIB -> RTC
                    let (ros_tx, _ros_rx) = mpsc::unbounded_channel();

                    if existing_topics.contains(&topic_gdp_name) {
                        info!("topic {:?} already exists in existing topics; don't need to create another subscriber", topic_gdp_name);
                    } else {

                        let channel_update_msg = FibStateChange {
                            action: FibChangeAction::ADD,
                            connection_type: FibConnectionType::SENDER, // sender because to send to webrtc
                            topic_gdp_name: topic_gdp_name,
                            forward_destination: Some(ros_tx),
                            description: Some("ros topic send".to_string()),
                        };
                        let _ = channel_tx.send(channel_update_msg);

                        existing_topics.push(topic_gdp_name);
                        let mut subscriber = manager_node.lock().unwrap()
                        .subscribe_untyped(&topic_name, &topic_type, r2r::QosProfile::default())
                        .expect("topic subscribing failure");


                        let fib_tx = fib_tx.clone();
                        let ros_handle = tokio::spawn(async move {
                            info!("ros_topic_remote_publisher ROS handling loop has started!");
                            while let Some(packet) = subscriber.next().await {
                                info!("received a ROS packet {:?}", packet);
                                let ros_msg = packet;
                                let packet = construct_gdp_forward_from_bytes(topic_gdp_name, self.unique_ros_node_gdp_name, ros_msg );
                                fib_tx.send(packet).expect("send for ros subscriber failure");
                            }
                        });
                        join_handles.push(ros_handle);
                    }
                }
            }
        }
        // tokio::join!(join_handles);
    }

    // fib -> ros topic locally
    pub async fn handle_ros_topic_remote_subscriber(
        self, mut status_recv: UnboundedReceiver<TopicManagerRequest>,
        fib_tx: UnboundedSender<GDPPacket>, channel_tx: UnboundedSender<FibStateChange>,
    ) {
        info!("ros_topic_remote_subscriber_handler has started");
        let mut join_handles = vec![];


        let ros_manager_node_clone = self.node.clone();
        let _handle = tokio::task::spawn_blocking(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
            ros_manager_node_clone
                .clone()
                .lock()
                .unwrap()
                .spin_once(std::time::Duration::from_millis(10));
        });

        let mut existing_topics = vec![];

        loop {
            tokio::select! {
                Some(request) = status_recv.recv() => {
                    let topic_name = request.topic_name;
                    let topic_type = request.topic_type;
                    let action = request.action;
                    let certificate = request.certificate;
                    let _fib_tx = fib_tx.clone();
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
                        error!("action {:?} not supported in ros_remote_service_provider", request.action);
                        continue;
                    }

                    // let stream = request.stream.unwrap();
                    let manager_node = self.node.clone();


                    info!(
                        "[ros_topic_remote_subscriber] topic creator for topic {}, type {}, action {:?}",
                        topic_name, topic_type, action
                    );


                    // RTC -> FIB -> ROS publisher
                    let (ros_tx, mut ros_rx) = mpsc::unbounded_channel();

                    let channel_update_msg = FibStateChange {
                        action: FibChangeAction::ADD,
                        connection_type: FibConnectionType::RECEIVER, // receiver because to receive from webrtc
                        topic_gdp_name: topic_gdp_name,
                        forward_destination: Some(ros_tx),
                        description: Some("ros topic receive".to_string()),
                    };
                    let _ = channel_tx.send(channel_update_msg);


                    // let rtc_handle = tokio::spawn(reader_and_writer(stream, fib_tx.clone(), rtc_rx));
                    // join_handles.push(rtc_handle);

                    if existing_topics.contains(&topic_gdp_name) {
                        info!("topic {:?} already exists in existing topics; don't need to create another publisher", topic_gdp_name);
                    } else {
                        existing_topics.push(topic_gdp_name);
                        let publisher = manager_node.lock().unwrap()
                        .create_publisher_untyped(&topic_name, &topic_type, r2r::QosProfile::default())
                        .expect("topic publisher create failure");

                        let ros_handle = tokio::spawn(async move {
                            info!("[ros_topic_remote_subscriber_handler] ROS handling loop for {} has started!", topic_name);
                            loop{
                                let pkt_to_forward = ros_rx.recv().await.expect("ros_topic_remote_subscriber_handler crashed!!");
                                if pkt_to_forward.action == GdpAction::Forward {
                                    info!("new payload to publish");
                                    if pkt_to_forward.gdpname == topic_gdp_name {
                                        let payload = pkt_to_forward.get_byte_payload().unwrap();
                                        //let ros_msg = serde_json::from_str(str::from_utf8(payload).unwrap()).expect("json parsing failure");
                                        // info!("the decoded payload to publish is {:?}", ros_msg);
                                        publisher.publish(payload.clone()).unwrap();
                                    } else{
                                        info!("{:?} received a packet for name {:?}",pkt_to_forward.gdpname, topic_gdp_name);
                                    }
                                }
                            }
                        });
                        join_handles.push(ros_handle);
                    }
                }
            }
        }
        // tokio::join!(join_handles);
    }

    // pub async fn handle_service_provider(
    //     &self,
    //     mut status_recv: UnboundedReceiver<TopicManagerRequest>,
    //     fib_tx: UnboundedSender<GDPPacket>,
    //     channel_tx: UnboundedSender<FibStateChange>,
    // ) {
    //     let mut join_handles = vec![];
    //     let mut existing_topics = vec![];

    //     while let Some(request) = status_recv.recv().await {
    //         let topic_name = request.topic_name;
    //         let topic_type = request.topic_type;
    //         let action = request.action;
    //         let certificate = request.certificate;
    //         let request_tx = fib_tx.clone();
    //         let topic_gdp_name = GDPName(get_gdp_name_from_topic(&topic_name, &topic_type, &certificate));

    //         if action != TopicManagerAction::ADD {
    //             error!("action {:?} not supported in ros_remote_service_provider", action);
    //             continue;
    //         }

    //         let manager_node = self.node.clone();
    //         info!(
    //             "[ros_topic_remote_publisher_handler] topic creator for topic {}, type {}, action {:?}",
    //             topic_name, topic_type, action
    //         );

    //         let (ros_tx, mut ros_rx) = mpsc::unbounded_channel();

    //         if existing_topics.contains(&topic_gdp_name) {
    //             info!("topic {:?} already exists in existing topics; don't need to create another subscriber", topic_gdp_name);
    //         } else {
    //             let channel_update_msg = FibStateChange {
    //                 action: FibChangeAction::ADD,
    //                 connection_type: FibConnectionType::RESPONSE,
    //                 topic_gdp_name: topic_gdp_name,
    //                 forward_destination: Some(ros_tx),
    //                 description: Some("ros service response".to_string()),
    //             };
    //             let _ = channel_tx.send(channel_update_msg);

    //             existing_topics.push(topic_gdp_name);
    //             let mut service = manager_node.lock().unwrap()
    //                 .create_service_untyped(&topic_name, &topic_type, r2r::QosProfile::default())
    //                 .expect("topic subscribing failure");

    //             let ros_handle = tokio::spawn(async move {
    //                 loop {
    //                     if let Some(req) = service.next().await {
    //                         let guid = format!("{:?}", req.request_id);
    //                         info!("received a ROS request {:?}", guid);
    //                         let packet_guid = generate_gdp_name_from_string(&guid);
    //                         let packet = construct_gdp_request_with_guid(topic_gdp_name, self.unique_ros_node_gdp_name.clone(), req.message.clone(), packet_guid);
    //                         request_tx.send(packet).expect("send for ros subscriber failure");

    //                         tokio::select! {
    //                             Some(packet) = ros_rx.recv() => {
    //                                 info!("received from webrtc in ros_rx {:?}", packet);
    //                                 let respond_msg = r2r::UntypedServiceSupport::new_from(&topic_type)
    //                                     .unwrap()
    //                                     .make_response_msg();
    //                                 respond_msg.from_binary(packet.payload.unwrap());
    //                                 info!("the decoded payload to publish is {:?}", respond_msg);
    //                                 req.respond(respond_msg).expect("could not send service response");
    //                             },
    //                             _ = tokio::time::sleep(Duration::from_millis(1000)) => {
    //                                 error!("timeout for ros_rx");
    //                                 let respond_msg = r2r::UntypedServiceSupport::new_from(&topic_type)
    //                                     .unwrap()
    //                                     .make_response_msg();
    //                                 req.respond(respond_msg).expect("could not send service response");
    //                             }
    //                         }
    //                     }
    //                 }
    //             });

    //             join_handles.push(ros_handle);
    //         }
    //     }
    // }
}
