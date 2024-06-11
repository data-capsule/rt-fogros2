use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use fogrs_common::packet_structs::{GDPPacket};
use fogrs_common::fib_structs::{FibStateChange};

pub struct ROSManager {
    // node: Arc<Mutex<r2r::Node>>,
    // unique_ros_node_gdp_name: GDPName,
}

impl ROSManager {
    pub fn new (node_name: &str, namespace: &str) -> Self {
        // let ctx = r2r::Context::create().expect("context creation failure");
        // let node = Arc::new(Mutex::new(Node::create(ctx, node_name, namespace).expect("node creation failure")));
        // let unique_ros_node_gdp_name = generate_random_gdp_name();

        // let ros_manager_node_clone = node.clone();
        // task::spawn_blocking(move || loop {
        //     std::thread::sleep(Duration::from_millis(100));
        //     ros_manager_node_clone.lock().unwrap().spin_once(Duration::from_millis(10));
        // });

        Self {
            // node,
            // unique_ros_node_gdp_name,
        }
    }
    
    // pub async fn handle_service_provider(
    //     &self,
    //     mut status_recv: UnboundedReceiver<TopicManagerRequest>,
    //     fib_tx: UnboundedSender<GDPPacket>,
    //     channel_tx: UnboundedSender<FibStateChange>,
    // ) {}
}


// struct ROSManager {
//     node: Arc<Mutex<r2r::Node>>,
//     unique_ros_node_gdp_name: GDPName,
// }

// impl ROSManager {
//     pub fn new(node_name: &str, namespace: &str) -> Self {
//         let ctx = r2r::Context::create().expect("context creation failure");
//         let node = Arc::new(Mutex::new(Node::create(ctx, node_name, namespace).expect("node creation failure")));
//         let unique_ros_node_gdp_name = generate_random_gdp_name();

//         let ros_manager_node_clone = node.clone();
//         task::spawn_blocking(move || loop {
//             std::thread::sleep(Duration::from_millis(100));
//             ros_manager_node_clone.lock().unwrap().spin_once(Duration::from_millis(10));
//         });

//         Self {
//             node,
//             unique_ros_node_gdp_name,
//         }
//     }

//     pub async fn handle_service_provider(
//         &self,
//         mut status_recv: UnboundedReceiver<TopicManagerRequest>,
//         fib_tx: UnboundedSender<GDPPacket>,
//         channel_tx: UnboundedSender<FibStateChange>,
//     ) {
//         let mut join_handles = vec![];
//         let mut existing_topics = vec![];

//         while let Some(request) = status_recv.recv().await {
//             let topic_name = request.topic_name;
//             let topic_type = request.topic_type;
//             let action = request.action;
//             let certificate = request.certificate;
//             let request_tx = fib_tx.clone();
//             let topic_gdp_name = GDPName(get_gdp_name_from_topic(&topic_name, &topic_type, &certificate));

//             if action != TopicManagerAction::ADD {
//                 error!("action {:?} not supported in ros_remote_service_provider", action);
//                 continue;
//             }

//             let manager_node = self.node.clone();
//             info!(
//                 "[ros_topic_remote_publisher_handler] topic creator for topic {}, type {}, action {:?}",
//                 topic_name, topic_type, action
//             );

//             let (ros_tx, mut ros_rx) = mpsc::unbounded_channel();

//             if existing_topics.contains(&topic_gdp_name) {
//                 info!("topic {:?} already exists in existing topics; don't need to create another subscriber", topic_gdp_name);
//             } else {
//                 let channel_update_msg = FibStateChange {
//                     action: FibChangeAction::ADD,
//                     connection_type: FibConnectionType::RESPONSE,
//                     topic_gdp_name: topic_gdp_name,
//                     forward_destination: Some(ros_tx),
//                     description: Some("ros service response".to_string()),
//                 };
//                 let _ = channel_tx.send(channel_update_msg);

//                 existing_topics.push(topic_gdp_name);
//                 let mut service = manager_node.lock().unwrap()
//                     .create_service_untyped(&topic_name, &topic_type, r2r::QosProfile::default())
//                     .expect("topic subscribing failure");

//                 let ros_handle = tokio::spawn(async move {
//                     loop {
//                         if let Some(req) = service.next().await {
//                             let guid = format!("{:?}", req.request_id);
//                             info!("received a ROS request {:?}", guid);
//                             let packet_guid = generate_gdp_name_from_string(&guid);
//                             let packet = construct_gdp_request_with_guid(topic_gdp_name, self.unique_ros_node_gdp_name.clone(), req.message.clone(), packet_guid);
//                             request_tx.send(packet).expect("send for ros subscriber failure");

//                             tokio::select! {
//                                 Some(packet) = ros_rx.recv() => {
//                                     info!("received from webrtc in ros_rx {:?}", packet);
//                                     let respond_msg = r2r::UntypedServiceSupport::new_from(&topic_type)
//                                         .unwrap()
//                                         .make_response_msg();
//                                     respond_msg.from_binary(packet.payload.unwrap());
//                                     info!("the decoded payload to publish is {:?}", respond_msg);
//                                     req.respond(respond_msg).expect("could not send service response");
//                                 },
//                                 _ = tokio::time::sleep(Duration::from_millis(1000)) => {
//                                     error!("timeout for ros_rx");
//                                     let respond_msg = r2r::UntypedServiceSupport::new_from(&topic_type)
//                                         .unwrap()
//                                         .make_response_msg();
//                                     req.respond(respond_msg).expect("could not send service response");
//                                 }
//                             }
//                         }
//                     }
//                 });

//                 join_handles.push(ros_handle);
//             }
//         }
//     }

//     // Similarly implement the other methods for `ros_local_service_caller`, `ros_topic_remote_publisher`, `ros_topic_remote_subscriber`
// }

