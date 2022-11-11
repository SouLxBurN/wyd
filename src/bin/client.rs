use wyd::client::connect_to_server;

#[tokio::main]
async fn main() {
    connect_to_server().await;
}
