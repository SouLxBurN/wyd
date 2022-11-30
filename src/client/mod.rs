use std::{env, io};

use futures_util::{future, pin_mut, StreamExt};
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

use crate::server::ChatMessage;

pub async fn connect_to_server(loc: String) {
    let connect_addr =
        env::args().nth(1).unwrap_or_else(|| panic!("Websocket connection string required."));
    let loc =
        env::args().nth(2).unwrap_or_else(|| panic!("Client Location is requied"));

    let url = url::Url::parse(&connect_addr).unwrap();

    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    println!("WebSocket handshake has been successfully completed");

    let (write, mut read) = ws_stream.split();

    let conn_resp = read.next().await.unwrap().unwrap();
    let client_id = conn_resp.to_string();

    let (stdin_tx, stdin_rx) = futures_channel::mpsc::unbounded();
    tokio::spawn(read_stdin_str(client_id, loc, stdin_tx));

    let stdin_to_ws = stdin_rx.map(Ok).forward(write);
    let ws_to_stdout = {
        read.for_each(|message| async {
            let data = message.unwrap().into_data();
            let msg: ChatMessage = serde_json::from_slice(&data).unwrap();
            tokio::io::stdout().write_all(
                format!("{}({}) | {}\n", msg.client_id, msg.loc.unwrap(), msg.message).as_bytes()
            ).await.unwrap();
        })
    };

    pin_mut!(stdin_to_ws, ws_to_stdout);
    future::select(stdin_to_ws, ws_to_stdout).await;
}

async fn read_stdin_str(client_id: String, loc: String, tx: futures_channel::mpsc::UnboundedSender<Message>) {
    let stdin = io::stdin();
    loop {
        let mut out = String::new();
        stdin.read_line(&mut out).expect("Failed to read user input");
        let cm = ChatMessage{
            client_id: client_id.clone(),
            message: out.trim().to_owned(),
            loc: Some(loc.clone()),
        };

        match serde_json::to_string(&cm) {
            Ok(msg) => {
                tx.unbounded_send(Message::text(msg)).unwrap();
            },
            Err(e) => eprintln!("{e}"),
        };
    }
}
