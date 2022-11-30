use std::env;

use wyd::client::connect_to_server;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    connect_to_server(args[1].clone()).await;
}
