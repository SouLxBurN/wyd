use futures_util::stream::SplitSink;
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
}

impl Client {
    pub fn new_client(id: String, addr: SocketAddr) -> Arc<Client> {
        Arc::new(Client{id, addr})
    }

    pub fn broadcast_listener(client: Arc<Client>, mut ws_write: SplitSink<WebSocketStream<TcpStream>, Message>, mut brx: Receiver<ChatMessage>) {
        tokio::spawn(async move {
            while let Ok(msg) = brx.recv().await {
                if client.id != msg.client_id {
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
}

pub struct BroadcastServer {
    active_clients: Vec<Client>,
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
            active_clients: vec!(),
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
                Self::accept_connection(stream, self.btx.clone())
            });
        }
    }

    pub async fn accept_connection(stream: TcpStream, btx: Sender<ChatMessage>) -> anyhow::Result<Arc<Client>> {
        let addr = stream.peer_addr().expect("connected streams should have a peer address");
        println!("Peer address: {}", addr);

        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .expect("Failed establish a connection");

        println!("Web Socket connection established {}", addr);

        let (mut write, read) = ws_stream.split();

        let now = SystemTime::now();
        let now = now.duration_since(UNIX_EPOCH).expect("This is bad").as_millis();
        let client = Client::new_client(now.to_string(), addr);
        write.send(Message::Text(client.clone().id.to_string())).await;

        Client::broadcast_listener(client.clone(), write, btx.subscribe());

        read.try_filter(|msg| future::ready(msg.is_text() || msg.is_binary()))
            .for_each(|msg| async {
                // "12345|This is my message"
                let raw = msg.unwrap().to_string();
                let mut split = raw.split("|");
                let cm = ChatMessage{
                    client_id: split.next().unwrap().to_string(),
                    message: split.next().unwrap().to_string(),
                };
                btx.send(cm);
            }).await;
        Ok(client)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = BroadcastServer::start();
    server.listen().await;

    Ok(())
}

