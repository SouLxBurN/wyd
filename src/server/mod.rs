mod client;
mod message;

use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::Receiver;
use tokio::sync::RwLock;
use tokio::sync::broadcast::{self, Sender};
use tokio::io::AsyncWriteExt;
use futures_util::{StreamExt, SinkExt};
use tokio_tungstenite::tungstenite::Message;
use std::collections::HashMap;
use std::sync::Arc;

use crate::server::client::Client;
pub use self::message::{Payload, ChatMessage, LocationMessage, PresenceMessage};

type ClientLocationMap = Arc<RwLock<HashMap<String, Vec<Arc<RwLock<Client>>>>>>;
type ActiveClientMap = Arc<RwLock<HashMap<String, Arc<RwLock<Client>>>>>;

pub struct BroadcastServer {
    active_clients: ActiveClientMap,
    btx: Sender<Payload>,
    disc_tx: tokio::sync::mpsc::Sender<String>,
    locations: ClientLocationMap,
    middleware: Vec<Box<dyn FnMut(Receiver<Payload>, Sender<Payload>)>>,
}

impl BroadcastServer {
    pub fn start() -> Self {
        let (btx, rtx) = broadcast::channel::<Payload>(16);
        let (disc_tx, mut r_disc_tx) = tokio::sync::mpsc::channel::<String>(6);

        let server = Self{
            active_clients: Arc::new(RwLock::new(HashMap::new())),
            btx,
            disc_tx,
            locations: Arc::new(RwLock::new(HashMap::new())),
            middleware: vec!(),
        };

        // Register a listener for the server to log messages
        tokio::spawn(
            BroadcastServer::internal_server_listener(
                rtx,
                server.active_clients.clone(),
                server.locations.clone())
        );

        // Listen for disconnect events to remove clients/victims
        let ac = server.active_clients.clone();
        let locs = server.locations.clone();
        tokio::spawn(async move {
            while let Some(victim) = r_disc_tx.recv().await {
                let mut client_map = ac.write().await;
                if let Some(client) = client_map.get(&victim) {
                    Self::remove_client_from_current_location(client.clone(), locs.clone()).await;
                }
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
                    self.disc_tx.clone(),
                    self.btx.clone())
            });
        }
    }

    pub async fn accept_connection(
        stream: TcpStream,
        clients: ActiveClientMap,
        disc_tx: tokio::sync::mpsc::Sender<String>,
        btx: Sender<Payload>) -> anyhow::Result<()> {

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
        let client = Client::new_client(client_id, addr, read, write, btx, disc_tx).await;

        let rwcl = client.read().await;
        let mut l = clients.write().await;
        l.insert(rwcl.id.clone(), client.clone());
        let client_id_msg = Message::Text(rwcl.id.clone());
        rwcl.ws_write.write().await.send(client_id_msg).await.unwrap();

        Ok(())
    }

    async fn internal_server_listener(
        mut rtx: tokio::sync::broadcast::Receiver<Payload>,
        active_clients: ActiveClientMap,
        locations: ClientLocationMap,
    ) {
        while let Ok(msg) = rtx.recv().await {
            let (client_id, location, out) = match msg {
                Payload::Chat(message) => {
                    let location = Self::get_active_location(&message.client_id, active_clients.clone()).await.unwrap_or_default();
                    (message.client_id, location, message.message)
                },
                Payload::Location(message) => {
                    let new_location = &message.location;
                    if let Some(client) = active_clients.read().await.get(&message.client_id) {
                        Self::remove_client_from_current_location(client.clone(), locations.clone()).await;
                        Self::add_client_to_location(client.clone(), &new_location, locations.clone()).await;
                        Self::send_location_response(client.clone(), &new_location, locations.clone()).await;
                        (message.client_id, new_location.to_owned(), message.location)
                    } else {
                        // If client isn't considered active, ignore request.
                        continue;
                    }
                },
                Payload::Presence(_message) => {
                    // Server shouldn't recieve presence payloads.
                    continue;
                },
            };

            // Temp Debugging lines for monitoring server state.
            // tokio::io::stdout().write_all(
            //     format!("clients: {:?}\n",
            //         active_clients.read().await,
            //     ).as_bytes()).await.unwrap();

            // tokio::io::stdout().write_all(
            //     format!("locations: {:?}\n",
            //         locations.read().await,
            //     ).as_bytes()).await.unwrap();

            tokio::io::stdout().write_all(
                format!("{}({})| {}\n",
                    client_id,
                    location,
                    out
                ).as_bytes()).await.unwrap();
        }
    }

    async fn get_active_location(client_id: &str, active_clients: ActiveClientMap) -> Option<String> {
        if let Some(client) = active_clients.read().await.get(client_id) {
            if let Some(loc) = &client.read().await.location {
                return Some(loc.to_string())
            }
        }
        None
    }

    async fn remove_client_from_current_location(client: Arc<RwLock<Client>>, locations: ClientLocationMap) {
        let rw_client = client.write().await;

        if let Some(client_location) = &rw_client.location {
            if let Some(location_clients) = locations.write().await.get_mut(client_location) {
                location_clients.retain_mut(|c| !Arc::ptr_eq(c, &client.clone()));
            }
        }
    }

    async fn add_client_to_location(client: Arc<RwLock<Client>>, location: &str, locations: ClientLocationMap) {
        let mut rw_client = client.write().await;

        rw_client.set_location(location.to_string());
        let mut rw_locations = locations.write().await;
        if let Some(location_clients) = rw_locations.get_mut(location) {
            location_clients.push(client.clone());
        } else {
            rw_locations.insert(location.to_string(), vec!(client.clone()));
        }
    }

    async fn send_location_response(client: Arc<RwLock<Client>>, location: &str, locations: ClientLocationMap) {
        let rw_locations = locations.write().await;

        if let Some(location_clients) = rw_locations.get(location) {
            let mut clients = Vec::new();
            for client in location_clients.iter() {
                clients.push(client.read().await.id.clone());
            };
            let response = PresenceMessage{
                clients,
                location: location.to_string(),
            };
            let rw_client = client.write().await;
            let _ = rw_client.ws_write.write().await.send(Message::Text(serde_json::to_string(&response).unwrap())).await;
        }
    }
}
