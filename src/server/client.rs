use tokio::sync::{Mutex, RwLock};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::broadcast::{Receiver, Sender};
use futures_util::{future, StreamExt, TryStreamExt, SinkExt};
use tokio_tungstenite::tungstenite::Message;
use futures_util::stream::{SplitSink, SplitStream};
use tokio::task::JoinHandle;
use std::net::SocketAddr;
use tokio_tungstenite::WebSocketStream;

use crate::server::message::ChatMessage;

pub type ClientID = String;

pub struct Client {
    pub id: ClientID,
    pub addr: SocketAddr,
    pub connected: bool,
    pub ws_write: Arc<Mutex<SplitSink<WebSocketStream<TcpStream>, Message>>>,
    bs_listener: Option<JoinHandle<()>>,
}

impl Client {
    pub async fn new_client(
        id: ClientID,
        addr: SocketAddr,
        ws_read: SplitStream<WebSocketStream<TcpStream>>,
        ws_write: SplitSink<WebSocketStream<TcpStream>, Message>,
        btx: Sender<ChatMessage>,
        dtx: tokio::sync::mpsc::Sender<ClientID>) -> Arc<RwLock<Client>> {

        let client = Arc::new(RwLock::new(
            Client{
                id,
                addr,
                connected: true,
                ws_write: Arc::new(Mutex::new(ws_write)),
                bs_listener: None
            }));

        Self::broadcast_listener(client.clone(), btx.subscribe()).await;
        Self::message_listener(client.clone(), ws_read, dtx, btx).await;

        client
    }

    async fn broadcast_listener(
        client: Arc<RwLock<Client>>,
        mut brx: Receiver<ChatMessage>) {

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
    }

    async fn message_listener(
        client: Arc<RwLock<Client>>,
        read: SplitStream<WebSocketStream<TcpStream>>,
        dtx: tokio::sync::mpsc::Sender<ClientID>,
        btx: Sender<ChatMessage>) {

        tokio::spawn(async move {
            read.try_filter(|msg| future::ready(msg.is_text() || msg.is_binary()))
                .for_each(|msg| async {
                    match msg {
                        Ok(raw) => {
                            let msg = raw.to_string();
                            let cm = ChatMessage{
                                client_id: client.read().await.id.clone(),
                                message: msg,
                            };
                            let __result = btx.send(cm);
                        },
                        Err(e) => {
                            let clr = &mut client.write().await;
                            if let Err(result) = dtx.send(clr.id.clone()).await {
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

    pub fn clean(&mut self) {
        self.connected = false;
        if let Some(listener) = self.bs_listener.take() {
            listener.abort();
        }
    }
}
