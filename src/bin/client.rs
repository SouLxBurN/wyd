use std::{env, io};

use futures_util::{future, pin_mut, StreamExt};
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

#[tokio::main]
async fn main() {
    let connect_addr =
        env::args().nth(1).unwrap_or_else(|| panic!("this program requires at least one argument"));

    let url = url::Url::parse(&connect_addr).unwrap();

    let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    println!("WebSocket handshake has been successfully completed");

    let (write, mut read) = ws_stream.split();

    let conn_resp = read.next().await.unwrap().unwrap();
    let client_id = conn_resp.to_string();

    let (stdin_tx, stdin_rx) = futures_channel::mpsc::unbounded();
    tokio::spawn(read_stdin_str(client_id, stdin_tx));

    let stdin_to_ws = stdin_rx.map(Ok).forward(write);
    let ws_to_stdout = {
        read.for_each(|message| async {
            let data = message.unwrap().into_data();
            tokio::io::stdout().write_all(&data).await.unwrap();
        })
    };

    pin_mut!(stdin_to_ws, ws_to_stdout);
    future::select(stdin_to_ws, ws_to_stdout).await;
}

async fn read_stdin_str(client_id: String, tx: futures_channel::mpsc::UnboundedSender<Message>) {
    let stdin = io::stdin();
    loop {
        let mut buf = String::new();
        stdin.read_line(&mut buf).expect("Failed to read user input");
        let out = client_id.clone() + "|" + &buf;

        tx.unbounded_send(Message::text(out)).unwrap();
    }
}
