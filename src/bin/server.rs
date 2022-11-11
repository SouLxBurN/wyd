use wyd::server::BroadcastServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = BroadcastServer::start();
    server.listen().await;
    Ok(())
}

