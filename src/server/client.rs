use tokio::sync::RwLock;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::broadcast::{Receiver, Sender};
use futures_util::{StreamExt, SinkExt};
use tokio_tungstenite::tungstenite::Message;
use futures_util::stream::{SplitSink, SplitStream};
use tokio::task::JoinHandle;
use std::net::SocketAddr;
use tokio_tungstenite::WebSocketStream;
use super::message::Payload;

pub type ClientID = String;

#[derive(Debug)]
pub struct Client {
    pub id: ClientID,
    pub location: Option<String>,
    pub addr: SocketAddr,
    pub connected: bool,
    pub ws_write: Arc<RwLock<SplitSink<WebSocketStream<TcpStream>, Message>>>,
    bs_listener: Option<JoinHandle<()>>,
}

impl Client {
    pub async fn new_client(
        id: ClientID,
        addr: SocketAddr,
        ws_read: SplitStream<WebSocketStream<TcpStream>>,
        ws_write: SplitSink<WebSocketStream<TcpStream>, Message>,
        btx: Sender<Payload>,
        disc_tx: tokio::sync::mpsc::Sender<ClientID>) -> Arc<RwLock<Client>> {

        let client = Arc::new(RwLock::new(
            Client{
                id,
                location: None,
                addr,
                connected: true,
                ws_write: Arc::new(RwLock::new(ws_write)),
                bs_listener: None
            }));

        Self::broadcast_listener(client.clone(), btx.subscribe()).await;
        Self::message_listener(client.clone(), ws_read, disc_tx, btx).await;

        client
    }

    /// A connected clients listener to the broadcast channel.
    /// Writes all messages to the clients write channel that
    /// do not match the client's id.
    async fn broadcast_listener(
        client: Arc<RwLock<Client>>,
        mut brx: Receiver<Payload>) {

        let local_client = client.clone();
        client.write().await.register_bs_listener(
            tokio::spawn(async move {
                while let Ok(msg) = brx.recv().await {
                    let cl = local_client.read().await;

                    if let Payload::Chat(cm) = msg {
                        if cl.id != cm.client_id {
                            let _write = cl.ws_write.write().await.send(
                                Message::text(serde_json::to_string(&cm).unwrap())
                            ).await;
                            // TODO Handle Errors: Backoff retries, before disconnecting.
                        }
                    }
                }
                eprintln!("broadcast_listener disconnected");
            })
        );
    }

    /// Spawns a message read task for a connected client's read stream.
    async fn message_listener(
        client: Arc<RwLock<Client>>,
        read: SplitStream<WebSocketStream<TcpStream>>,
        disc_tx: tokio::sync::mpsc::Sender<ClientID>,
        btx: Sender<Payload>) {

        tokio::spawn(async move {
            read.for_each(|msg| async {
                match msg {
                    Ok(raw) => {
                        let msg = raw.to_string();
                        let payload: Payload = serde_json::from_str(&msg).unwrap();
                        btx.send(payload).unwrap();
                        // TODO Validate the client id.
                    },
                    Err(e) => {
                        let clr = &mut client.write().await;
                        if let Err(result) = disc_tx.send(clr.id.clone()).await {
                            eprintln!("Failed to {}", result);
                        }
                        clr.clean();
                        eprintln!("Client {} has disconnected: {e}", &clr.id);
                    },
                }
            }).await;
        });
    }

    fn register_bs_listener(&mut self, bs_listener: JoinHandle<()>) {
        self.bs_listener = Some(bs_listener);
    }

    pub fn set_location(&mut self, location: String) {
        self.location = Some(location);
    }

    pub fn clean(&mut self) {
        self.connected = false;
        if let Some(listener) = self.bs_listener.take() {
            listener.abort();
        }
    }
}
