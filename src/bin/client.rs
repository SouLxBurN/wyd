use std::env;

use wyd::client::{connect_to_server, ConnectionDetails};

#[tokio::main]
async fn main() {
    let mut args = env::args();
    let connect_addr = args
        .nth(1)
        .unwrap_or_else(|| panic!("Websocket connection string required."));
    let loc = args
        .next()
        .unwrap_or_else(|| panic!("Initial location required"));

    let details = ConnectionDetails {
        addr: connect_addr,
        init_location: loc,
    };

    connect_to_server(details).await;
}
