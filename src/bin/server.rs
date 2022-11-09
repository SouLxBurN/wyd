use futures_util::stream::SplitSink;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast::{self, Sender, Receiver};
use tokio::io::AsyncWriteExt;
use futures_util::{future, StreamExt, TryStreamExt, SinkExt};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;

#[derive(Clone)]
pub struct ChatMessage {
    client_id: String,
    message: String,
}

pub struct Client {
    id: String,
    addr: SocketAddr,
    connected: bool,
}

impl Client {
    pub fn new_client(id: String, addr: SocketAddr) -> Arc<RwLock<Client>> {
        Arc::new(RwLock::new(Client{id, addr, connected: true}))
    }

    pub fn broadcast_listener(client: Arc<RwLock<Client>>, mut ws_write: SplitSink<WebSocketStream<TcpStream>, Message>, mut brx: Receiver<ChatMessage>) {
        tokio::spawn(async move {
            while let Ok(msg) = brx.recv().await {
                let cl = client.read().await;
                if !cl.connected {
                    break;
                }
                if cl.id != msg.client_id {
                    ws_write.send(
                        Message::text(
                            format!("{}|{}",
                                msg.client_id, msg.message))
                    ).await;
                }
            }
            eprintln!("broadcast_listener disconnected");
        });
    }

    pub fn clean(&mut self) {
        self.connected = false;
    }
}

pub struct BroadcastServer {
    active_clients: Arc<Mutex<HashMap<String, Arc<RwLock<Client>>>>>,
    btx: Sender<ChatMessage>,
}

impl BroadcastServer {
    pub fn start() -> Self {
        let (btx, mut rtx) = broadcast::channel::<ChatMessage>(16);
        tokio::spawn(async move {
            while let Ok(msg) = rtx.recv().await {
                tokio::io::stdout().write_all(
                    format!("{} | {}",
                        msg.client_id,
                        msg.message
                  ).as_bytes()).await.unwrap();
            }
        });
        Self{
            active_clients: Arc::new(Mutex::new(HashMap::new())),
            btx,
        }
    }

    pub async fn listen(&self) {
        let addr = "127.0.0.1:8080";
        let t_sock = TcpListener::bind(&addr).await;
        let listener = t_sock.expect("Failed to bind");

        println!("Listening on {}", addr);
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn({
                Self::accept_connection(stream, self.active_clients.clone(), self.btx.clone())
            });
        }
    }

    pub async fn accept_connection(stream: TcpStream, clients: Arc<Mutex<HashMap<String, Arc<RwLock<Client>>>>>, btx: Sender<ChatMessage>) -> anyhow::Result<()> {
        let addr = stream.peer_addr().expect("connected streams should have a peer address");
        println!("Peer address: {}", addr);

        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .expect("Failed establish a connection");

        println!("Web Socket connection established {}", addr);

        let (mut write, read) = ws_stream.split();

        let now = SystemTime::now();
        let now = now.duration_since(UNIX_EPOCH).expect("This is bad").as_millis();
        let client_id = now.to_string();
        let client = Client::new_client(client_id, addr);

        {
            let rwcl = client.read().await;
            write.send(Message::Text(rwcl.id.clone())).await;
            let mut l = clients.lock().await;
            l.insert(rwcl.id.clone(), client.clone());
        }

        Client::broadcast_listener(client.clone(), write, btx.subscribe());
        read.try_filter(|msg| future::ready(msg.is_text() || msg.is_binary()))
            .for_each(|msg| async {
                // "12345|This is my message"
                match msg {
                    Ok(raw) => {
                        let raw = raw.to_string();
                        let mut split = raw.split("|");
                        let cm = ChatMessage{
                            client_id: split.next().unwrap().to_string(),
                            message: split.next().unwrap().to_string(),
                        };
                        btx.send(cm);
                    },
                    Err(e) => {
                        let mut client_map = clients.lock().await;
                        let cl = &mut client.write().await;
                        client_map.remove(&cl.id);
                        cl.clean();
                        eprintln!("Client {} has disconnected: {e}", &cl.id);
                    },
                }
            }).await;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = BroadcastServer::start();
    server.listen().await;

    Ok(())
}

