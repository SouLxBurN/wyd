mod client;
mod message;

use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, RwLock};
use tokio::sync::broadcast::{self, Sender};
use tokio::io::AsyncWriteExt;
use futures_util::{StreamExt, SinkExt};
use tokio_tungstenite::tungstenite::Message;
use std::collections::HashMap;
use std::sync::Arc;

use crate::server::client::Client;
use crate::server::message::ChatMessage;

pub struct BroadcastServer {
    active_clients: Arc<Mutex<HashMap<String, Arc<RwLock<Client>>>>>,
    btx: Sender<ChatMessage>,
    dtx: tokio::sync::mpsc::Sender<String>,
}

impl BroadcastServer {
    pub fn start() -> Self {
        let (btx, mut rtx) = broadcast::channel::<ChatMessage>(16);
        let (dtx, mut rdtx) = tokio::sync::mpsc::channel::<String>(6);

        let server = Self{
            active_clients: Arc::new(Mutex::new(HashMap::new())),
            btx,
            dtx,
        };

        // Register a listener for the server to log messages
        tokio::spawn(async move {
            while let Ok(msg) = rtx.recv().await {
                tokio::io::stdout().write_all(
                    format!("{} | {}",
                        msg.client_id,
                        msg.message
                  ).as_bytes()).await.unwrap();
            }
        });

        // Listen for disconnect events to remove clients/victims
        let ac = server.active_clients.clone();
        tokio::spawn(async move {
            while let Some(victim) = rdtx.recv().await {
                let mut client_map = ac.lock().await;
                client_map.remove(&victim);
            }
        });

        server
    }

    pub async fn listen(&self) {
        let addr = "127.0.0.1:8080";
        let t_sock = TcpListener::bind(&addr).await;
        let listener = t_sock.expect("Failed to bind");

        println!("Listening on {}", addr);
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn({
                Self::accept_connection(
                    stream,
                    self.active_clients.clone(),
                    self.dtx.clone(),
                    self.btx.clone())
            });
        }
    }

    pub async fn accept_connection(
        stream: TcpStream,
        clients: Arc<Mutex<HashMap<String, Arc<RwLock<Client>>>>>,
        dtx: tokio::sync::mpsc::Sender<String>,
        btx: Sender<ChatMessage>) -> anyhow::Result<()> {

        let addr = stream.peer_addr().expect("connected streams should have a peer address");
        println!("Peer address: {}", addr);

        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .expect("Failed establish a connection");
        println!("Web Socket connection established {}", addr);

        let (write, read) = ws_stream.split();

        let now = SystemTime::now();
        let now = now.duration_since(UNIX_EPOCH).expect("This is bad").as_millis();
        let client_id = now.to_string();
        let client = Client::new_client(client_id, addr, read, write, btx, dtx).await;

        let rwcl = client.read().await;
        let mut l = clients.lock().await;
        l.insert(rwcl.id.clone(), client.clone());
        let client_id_msg = Message::Text(rwcl.id.clone());
        rwcl.ws_write.lock().await.send(client_id_msg).await.unwrap();

        Ok(())
    }
}
