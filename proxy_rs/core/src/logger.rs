use tokio::sync::mpsc::UnboundedSender;

use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::UnboundedReceiver;

pub struct Logger {
    sender: UnboundedSender<String>,
}

impl Logger {
    pub fn new(sender: UnboundedSender<String>) -> Self {
        Logger { sender }
    }

    pub fn log(&self, message: String) {
        let _ = self.sender.send(message);
    }
}

pub async fn handle_logs(mut receiver: UnboundedReceiver<String>, file_path: String) {
    let mut file = OpenOptions::new()
        .append(true)
        .create(true)
        .open(file_path.as_str())
        .await
        .expect("file creation failure");

    while let Some(message) = receiver.recv().await {
        file.write_all(message.as_bytes()).await;
        file.write_all(b"\n").await;
    }
}
