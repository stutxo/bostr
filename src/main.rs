use bitcoin::consensus::Decodable;

use std::thread;

use bitcoin::Transaction;
use nostr_sdk::prelude::*;
use tokio::sync::mpsc;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().without_time().init();

    let (tx, rx) = mpsc::channel(100);
    let nostr_task = tokio::spawn(nostr_task(rx));

    thread::spawn(move || zmq_task(tx));

    if let Err(e) = tokio::try_join!(nostr_task) {
        error!("Task exited unexpectedly: {:?}", e);
    }
}

async fn nostr_task(mut rx: mpsc::Receiver<Vec<u8>>) {
    let client = Client::new(Keys::generate());
    client.add_relay("wss://relay.damus.io").await.unwrap();
    client.add_relay("wss://nostr.wine").await.unwrap();
    client.add_relay("wss://nos.lol").await.unwrap();
    client.connect().await;

    let bitcoin_tx_kind = Kind::Custom(28333);

    let filter = Filter::new().kind(bitcoin_tx_kind).since(Timestamp::now());

    let magic_value = "f9beb4d9".to_string(); // Mainnet yolo

    let client_clone = client.clone();
    tokio::spawn(async move {
        while let Some(tx_data) = rx.recv().await {
            let tx_hex = hex::encode(&tx_data);

            let magic_tag = Tag::custom(TagKind::Custom("magic".into()), vec![magic_value.clone()]);

            let transactions_tag =
                Tag::custom(TagKind::Custom("transactions".into()), vec![tx_hex.clone()]);

            let ephemeral_keys = Keys::generate();

            let event_builder = EventBuilder::new(
                bitcoin_tx_kind,
                "", // Content MUST be empty per NIP-89
            )
            .tags(vec![magic_tag, transactions_tag]);

            match event_builder.sign(&ephemeral_keys).await {
                Ok(event) => {
                    let event_id = event.id;
                    match client_clone.send_event(event).await {
                        Ok(_) => info!("Published NIP-89 event: {}", event_id),
                        Err(e) => error!("Failed to publish event {}: {:?}", event_id, e),
                    }
                }
                Err(e) => error!("Failed to sign event: {:?}", e),
            }
        }
    });

    let sub_id = client.subscribe(filter, None).await.unwrap().val;

    client
        .handle_notifications(|n| async {
            if let RelayPoolNotification::Event {
                subscription_id,
                event,
                ..
            } = n
            {
                if subscription_id == sub_id {
                    for tag in event.tags {
                        if tag.kind() == TagKind::Custom("transactions".into()) {
                            for hex_tx in tag.to_vec().iter().skip(1) {
                                if let Ok(tx_bytes) = hex::decode(hex_tx) {
                                    if let Ok(tx) =
                                        Transaction::consensus_decode(&mut tx_bytes.as_slice())
                                    {
                                        info!(
                                            "Received transaction {} from Nostr",
                                            tx.compute_txid()
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Ok(false)
        })
        .await
        .unwrap();
}

fn zmq_task(tx: mpsc::Sender<Vec<u8>>) {
    let endpoint = "tcp://127.0.0.1:28332";
    let ctx = zmq::Context::new();
    let socket = ctx.socket(zmq::SUB).expect("ZMQ socket failed");
    socket.connect(endpoint).expect("ZMQ connect failed");
    socket
        .set_subscribe(b"rawtx")
        .expect("ZMQ subscribe failed");

    info!("[ZMQ] Listening for transactions...");
    loop {
        if let Ok(Ok(topic)) = socket.recv_string(0) {
            info!("[ZMQ] Received topic: {}", topic);
            if topic == "rawtx" {
                if let Ok(payload) = socket.recv_bytes(0) {
                    info!("[ZMQ] Received raw transaction of {} bytes", payload.len());
                    let _ = socket.recv_bytes(0); // seq
                    let _ = tx.blocking_send(payload);
                }
            }
        }
    }
}
