
use std::sync::Arc;

use fogrs_signaling::Server;
use env_logger;
use console_subscriber;

#[tokio::main]
async fn main() {

    env_logger::builder().format_timestamp_micros().init();
    // console_subscriber::init();
    let server = Arc::new(Server::new());
    server.run("0.0.0.0:8080").await;
}