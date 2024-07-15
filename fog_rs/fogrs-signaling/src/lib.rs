use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, Mutex};
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio_stream::StreamExt;
use serde::{Serialize, Deserialize};
use serde_json;

#[derive(Serialize, Deserialize)]
pub struct Message {
    command: String,
    topic: String,
    data: Option<String>,
}

#[derive(Clone)]
pub struct Topic {
    name: String,
    log: Arc<Mutex<VecDeque<(String, tokio::time::Instant)>>>,
    sender: broadcast::Sender<String>,
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

    async fn publish(&self, message: String) {
        let mut log = self.log.lock().await;
        log.push_back((message.clone(), tokio::time::Instant::now()));
        self.sender.send(message).unwrap();
    }

    async fn subscribe(&self, mut stream: TcpStream) {
        let mut rx = self.sender.subscribe();
        let log = self.log.lock().await.clone();

        // Stream the log to the subscriber
        for (msg, _) in log {
            let response = serde_json::to_string(&msg).unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
        }

        // Stream new messages to the subscriber
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    let response = serde_json::to_string(&msg).unwrap();
                    stream.write_all(response.as_bytes()).await.unwrap();
                }
                Err(_) => break,
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
        stream.read(&mut buffer).await.unwrap();
        let request = String::from_utf8(buffer.to_vec()).unwrap();
        let message: Message = serde_json::from_str(&request).unwrap();

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
            _ => {}
        }
    }

    pub async fn run(self: Arc<Self>, addr: &str) {
        let listener = TcpListener::bind(addr).await.unwrap();

        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let server = self.clone();
            tokio::spawn(async move {
                server.handle_client(stream).await;
            });
        }
    }
}

pub async fn publish(topic: &str, message: &str) -> io::Result<()> {
    let mut stream = TcpStream::connect("127.0.0.1:8080").await?;
    let request = Message {
        command: "PUBLISH".to_string(),
        topic: topic.to_string(),
        data: Some(message.to_string()),
    };
    let request = serde_json::to_string(&request).unwrap();
    stream.write_all(request.as_bytes()).await?;
    Ok(())
}

pub async fn subscribe(topic: &str) -> io::Result<()> {
    let mut stream = TcpStream::connect("127.0.0.1:8080").await?;
    let request = Message {
        command: "SUBSCRIBE".to_string(),
        topic: topic.to_string(),
        data: None,
    };
    let request = serde_json::to_string(&request).unwrap();
    stream.write_all(request.as_bytes()).await?;

    let mut buffer = [0; 1024];
    loop {
        let n = stream.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        println!("{}", String::from_utf8_lossy(&buffer[..n]));
    }
    Ok(())
}
