use tokio::sync::{Mutex, RwLock};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::broadcast::Receiver;
use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::Message;
use futures_util::stream::SplitSink;
use tokio::task::JoinHandle;
use std::net::SocketAddr;
use tokio_tungstenite::WebSocketStream;

use crate::server::message::ChatMessage;

pub struct Client {
    pub id: String,
    pub addr: SocketAddr,
    pub connected: bool,
    pub ws_write: Arc<Mutex<SplitSink<WebSocketStream<TcpStream>, Message>>>,
    bs_listener: Option<JoinHandle<()>>,
}

impl Client {
    pub async fn new_client(
        id: String,
        addr: SocketAddr,
        ws_write: SplitSink<WebSocketStream<TcpStream>, Message>,
        brx: Receiver<ChatMessage>) -> Arc<RwLock<Client>> {

        let client = Arc::new(RwLock::new(
            Client{
                id,
                addr,
                connected: true,
                ws_write: Arc::new(Mutex::new(ws_write)),
                bs_listener: None
            }));

        Self::broadcast_listener(client, brx).await
    }

    async fn broadcast_listener(client: Arc<RwLock<Client>>, mut brx: Receiver<ChatMessage>) -> Arc<RwLock<Client>> {
        let local_client = client.clone();

        client.write().await.register_bs_listener(
            tokio::spawn(async move {
                while let Ok(msg) = brx.recv().await {
                    let cl = local_client.read().await;
                    if cl.id != msg.client_id {
                        let _write = cl.ws_write.lock().await.send(
                            Message::text(
                                format!("{} | {}",
                                    msg.client_id, msg.message))
                        ).await;
                        // TODO Handle Errors: Backoff retries, before disconnecting.
                    }
                }
                eprintln!("broadcast_listener disconnected");
            })
        );
        client
    }

    fn register_bs_listener(&mut self, bs_listener: JoinHandle<()>) {
        self.bs_listener = Some(bs_listener);
    }

    pub fn clean(&mut self) {
        self.connected = false;
        if let Some(listener) = self.bs_listener.take() {
            listener.abort();
        }
    }
}
