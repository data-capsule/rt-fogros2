
use std::sync::Arc;

use fogrs_signaling::Server;
use env_logger;

#[tokio::main]
async fn main() {
    env_logger::builder().format_timestamp_micros().init();
    let server = Arc::new(Server::new());
    server.run("0.0.0.0:8080").await;
}