use anyhow::{Context, anyhow};
use base64::prelude::*;
use bitcoin::{Transaction, consensus::Decodable};
use bitcoind_async_client::Client as BitcoinClient;
use bitcoind_async_client::traits::Broadcaster;
use clap::Parser;
use nostr_sdk::prelude::*;
use std::fs;
use std::thread;
use tokio::sync::mpsc;
use tracing::{error, info};

#[derive(Parser, Debug)]
struct Cli {
    #[arg(long)]
    zmq: String,
    #[arg(long)]
    bitcoinrpc: String,
    #[arg(long)]
    datadir: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().without_time().init();
    let args = Cli::parse();

    let cookie_path = format!("{}/.cookie", args.datadir);
    let cookie_content = fs::read_to_string(&cookie_path)
        .with_context(|| format!("Failed to read cookie file at {}", cookie_path))
        .unwrap();

    let (user, password) = cookie_content
        .trim()
        .split_once(':')
        .ok_or_else(|| anyhow!("Invalid cookie format"))
        .unwrap();

    let bitcoin_rpc = BitcoinClient::new(
        args.bitcoinrpc.to_string(),
        user.to_string(),
        password.to_string(),
        None,
        None,
    )
    .map_err(|e| anyhow!("Failed to create client: {}", e))
    .unwrap();

    let (tx, rx) = mpsc::channel(1000);

    let nostr_handle = tokio::spawn(nostr_task(rx, bitcoin_rpc));

    let zmq_args = args.zmq.clone();
    thread::spawn(move || zmq_task(tx, zmq_args));

    if let Err(e) = nostr_handle.await {
        error!("Nostr task failed: {:?}", e);
    }
}

async fn nostr_task(mut rx: mpsc::Receiver<Vec<u8>>, bitcoin_rpc: BitcoinClient) {
    let client = Client::new(Keys::generate());
    client.add_relay("wss://relay.damus.io").await.unwrap();
    client.add_relay("wss://nostr.wine").await.unwrap();
    client.add_relay("wss://nos.lol").await.unwrap();
    client.connect().await;

    let client_rx = client.clone();
    let bitcoin_tx_kind = Kind::Custom(28333);

    tokio::spawn(async move {
        let filter = Filter::new().kind(bitcoin_tx_kind).since(Timestamp::now());
        client_rx.subscribe(filter, None).await.unwrap();

        client_rx
            .handle_notifications(|n| async {
                if let RelayPoolNotification::Event { event, .. } = n {
                    for tag in event.tags {
                        if tag.kind() == TagKind::Custom("transactions".into()) {
                            for encoded_tx in tag.to_vec().iter().skip(1) {
                                if let Ok(tx_bytes) = BASE64_STANDARD.decode(encoded_tx) {
                                    if let Ok(tx) =
                                        Transaction::consensus_decode(&mut tx_bytes.as_slice())
                                    {
                                        info!(">> Received TXID: {}", tx.compute_txid());

                                        match bitcoin_rpc.send_raw_transaction(&tx).await {
                                            Ok(_) => info!("Submitted to node"),
                                            Err(e) => {
                                                error!("Failed to submit transaction: {:?}", e)
                                            }
                                        }
                                    } else {
                                        error!("Failed to decode transaction");
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
    });

    let mut buffer: Vec<String> = Vec::new();
    let mut current_size = 0;
    const MAX_SIZE: usize = 10_000;

    info!("Waiting for transactions to fill buffer...");

    while let Some(tx_bytes) = rx.recv().await {
        let tx_b64 = BASE64_STANDARD.encode(tx_bytes);

        info!("Received transaction, size: {} bytes", tx_b64.len());

        let tx_len = tx_b64.len();

        buffer.push(tx_b64);
        current_size += tx_len;

        if current_size >= MAX_SIZE {
            info!(
                "Buffer full ({} bytes). Sending {} transactions...",
                current_size,
                buffer.len()
            );

            let tx_tag = Tag::custom(TagKind::Custom("transactions".into()), buffer.clone());
            let magic_tag = Tag::custom(
                TagKind::Custom("magic".into()),
                vec!["f9beb4d9".to_string()],
            );

            let event = EventBuilder::new(bitcoin_tx_kind, "").tags(vec![magic_tag, tx_tag]);

            if let Ok(signed_event) = event.sign(&Keys::generate()).await {
                if let Err(e) = client.send_event(signed_event).await {
                    error!("Publish failed: {:?}", e);
                } else {
                    info!("Successfully published batch!");
                }
            }

            buffer.clear();
            current_size = 0;
        }
    }
}

fn zmq_task(tx: mpsc::Sender<Vec<u8>>, zmq_address: String) {
    let ctx = zmq::Context::new();
    let socket = ctx.socket(zmq::SUB).expect("Socket failed");
    socket.connect(&zmq_address).expect("Connect failed");
    socket.set_subscribe(b"rawtx").expect("Subscribe failed");

    loop {
        if let Ok(Ok(topic)) = socket.recv_string(0) {
            if topic == "rawtx" {
                if let Ok(payload) = socket.recv_bytes(0) {
                    let _ = socket.recv_bytes(0); // skip sequence
                    let _ = tx.blocking_send(payload);
                }
            }
        }
    }
}
