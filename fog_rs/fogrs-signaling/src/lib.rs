use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use serde_json;
use log::{error, info, warn};
use std::io;
use serde::{Serialize, Deserialize};
use fogrs_common::fib_structs::CandidateStruct;

#[derive(Serialize, Deserialize, Debug)]
pub struct Message {
    pub command: String,
    pub topic: String,
    pub data: Option<CandidateStruct>,
}

#[derive(Clone)]
pub struct Topic {
    pub name: String,
    pub log: Arc<Mutex<VecDeque<(CandidateStruct, tokio::time::Instant)>>>,
    pub sender: broadcast::Sender<CandidateStruct>,
}

impl Topic {
    fn new(name: &str) -> Self {
        let (sender, _) = broadcast::channel(100);
        Topic {
            name: name.to_string(),
            log: Arc::new(Mutex::new(VecDeque::new())),
            sender,
        }
    }

    async fn publish(&self, message: CandidateStruct) {
        info!("Publishing message to topic {}: {:?}", self.name, message);
        info!("current log: {:?}", self.log.lock().await);
        let mut log = self.log.lock().await;
        log.push_back((message.clone(), tokio::time::Instant::now()));
        if self.sender.receiver_count() > 0 {
            if let Err(e) = self.sender.send(message) {
                error!("Error broadcasting message: {}", e);
            }
        } else {
            warn!("No subscribers to topic {}", self.name);
        }
    }

    async fn subscribe(&self, mut stream: TcpStream) {
        info!("New subscriber to topic {}", self.name);
        let mut rx = self.sender.subscribe();
        let log = self.log.lock().await.clone();

        // Stream the log to the subscriber
        for (msg, _) in log {
            let response = serde_json::to_string(&msg).unwrap();
            if let Err(e) = stream.write_all(response.as_bytes()).await {
                error!("Error sending log message: {}", e);
                return;
            }
        }

        // Stream new messages to the subscriber
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let response = serde_json::to_string(&msg).unwrap();
                    if let Err(e) = stream.write_all(response.as_bytes()).await {
                        error!("Error sending TCP message: {}", e);
                        return;
                    }
                }
                Err(_) => {
                    warn!("Subscriber disconnected from topic {}", self.name);
                },
            }
        }
    }
}

pub struct Server {
    pub topics: Arc<Mutex<HashMap<String, Topic>>>,
}

impl Server {
    pub fn new() -> Self {
        Server {
            topics: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn handle_client(self: Arc<Self>, mut stream: TcpStream) {
        let mut buffer = [0; 1024];
        if let Err(e) = stream.read(&mut buffer).await {
            error!("Error reading from stream: {}", e);
            return;
        }
        info!("Received request: {}", String::from_utf8_lossy(&buffer));
        let request = match String::from_utf8_lossy(&buffer).trim_matches(char::from(0)) {
            "" => return,
            s => s.to_string(),
        };

        let message: Message = match serde_json::from_str(&request) {
            Ok(msg) => msg,
            Err(e) => {
                error!("Error deserializing message: {}", e);
                return;
            }
        };

        let mut topics = self.topics.lock().await;

        match message.command.as_str() {
            "PUBLISH" => {
                if let Some(topic) = topics.get(&message.topic) {
                    topic.publish(message.data.unwrap()).await;
                } else {
                    let topic = Topic::new(&message.topic);
                    topic.publish(message.data.unwrap()).await;
                    topics.insert(message.topic.clone(), topic);
                }
            }
            "SUBSCRIBE" => {
                if let Some(topic) = topics.get(&message.topic) {
                    let topic = topic.clone();
                    drop(topics);
                    topic.subscribe(stream).await;
                } else {
                    let topic = Topic::new(&message.topic);
                    topics.insert(message.topic.clone(), topic.clone());
                    drop(topics);
                    topic.subscribe(stream).await;
                }
            }
            _ => {
                error!("Unknown command: {}", message.command);
            }
        }
    }

    pub async fn run(self: Arc<Self>, addr: &str) {
        let listener = TcpListener::bind(addr).await.unwrap();
        info!("Server running on {}", addr);

        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let server = self.clone();
            tokio::spawn(async move {
                server.handle_client(stream).await;
            });
        }
    }
}
// pub async fn publish(topic: &str, message: &str) -> io::Result<()> {
//     let mut stream = TcpStream::connect("127.0.0.1:8080").await?;
//     let request = Message {
//         command: "PUBLISH".to_string(),
//         topic: topic.to_string(),
//         data: Some(message.to_string()),
//     };
//     let request = serde_json::to_string(&request).unwrap();
//     stream.write_all(request.as_bytes()).await?;
//     Ok(())
// }

// pub async fn subscribe(topic: &str) -> io::Result<()> {
//     let mut stream = TcpStream::connect("127.0.0.1:8080").await?;
//     let request = Message {
//         command: "SUBSCRIBE".to_string(),
//         topic: topic.to_string(),
//         data: None,
//     };
//     let request = serde_json::to_string(&request).unwrap();
//     stream.write_all(request.as_bytes()).await?;

//     let mut buffer = [0; 1024];
//     loop {
//         let n = stream.read(&mut buffer).await?;
//         if n == 0 {
//             break;
//         }
//         println!("{}", String::from_utf8_lossy(&buffer[..n]));
//     }
//     Ok(())
// }

