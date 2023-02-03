use std::io;

use futures_util::stream::SplitSink;
use futures_util::{future, pin_mut, SinkExt, StreamExt};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use crate::client::Payload::{Chat, Location, Presence};
use crate::server::{Payload, ChatMessage, LocationMessage};

pub struct ConnectionDetails {
    pub addr: String,
    pub init_location: String,
}

pub async fn connect_to_server(details: ConnectionDetails) {
    let url = url::Url::parse(&details.addr).unwrap();

    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    println!("WebSocket handshake has been successfully completed");

    let (mut write, mut read) = ws_stream.split();

    let conn_resp = read.next().await.unwrap().unwrap();
    let client_id = conn_resp.to_string();
    send_location(client_id.clone(), details.init_location, &mut write).await;

    let (stdin_tx, stdin_rx) = futures_channel::mpsc::unbounded();
    tokio::spawn(read_stdin_str(client_id, stdin_tx));

    let stdin_to_ws = stdin_rx.map(Ok).forward(write);
    let ws_to_stdout = {
        read.for_each(|message| async {
            let data = message.unwrap().into_data();
            let msg: Payload = serde_json::from_slice(&data).unwrap();

            let output = match msg {
                Chat(message) => {
                    format!("{}| {}\n", message.client_id, message.message)
                },
                Location(_message) => {
                    format!("Why did I get a location message?\n")
                },
                Presence(message) => {
                    format!("Clients: {:?}\n", message.clients)
                },
            };

            tokio::io::stdout()
                .write_all(output.as_bytes())
                .await
                .unwrap()
        })
    };

    pin_mut!(stdin_to_ws, ws_to_stdout);
    future::select(stdin_to_ws, ws_to_stdout).await;
}

async fn send_location(
    client_id: String,
    location: String,
    write: &mut SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>,
) {
    let init_loc = LocationMessage {
        client_id,
        location,
    };
    let _out = write
        .send(Message::text(serde_json::to_string(&init_loc).unwrap()))
        .await;
}

async fn read_stdin_str(client_id: String, tx: futures_channel::mpsc::UnboundedSender<Message>) {
    let stdin = io::stdin();
    loop {
        let mut out = String::new();
        stdin
            .read_line(&mut out)
            .expect("Failed to read user input");

        let outbound: String = if let Some(location) = out.strip_prefix("loc:") {
            let lm = LocationMessage {
                client_id: client_id.clone(),
                location: location.trim().to_owned(),
            };
            serde_json::to_string(&lm).unwrap()
        } else {
            let cm = ChatMessage {
                client_id: client_id.clone(),
                message: out.trim().to_owned(),
            };
            serde_json::to_string(&cm).unwrap()
        };
        tx.unbounded_send(Message::text(outbound)).unwrap();
    }
}
