# bitcoin and other stuff transmitted by relays (BOSTR) (PoC)

## Plebfi dec 7th 2025 hackathon submission

We use ZMQ and nostr to relay bitcoin transactions between bitcoin nodes

uses ben carmans "nip-85" https://github.com/nostr-protocol/nips/pull/476

Bostr will listen to the `rawtx` bitcoin core ZMQ topic and forward transactions over nostr using kind(28333)

If there is another Bostr node running somewhere, it will recieve the ephemeral note containing the batched bitcoin transacations and submit the raw transactions to its node

# How i setup the demo

```
./build/bin/bitcoind -datadir=<datadir1> -zmqpubrawtx=tcp://127.0.0.1:28332
```

blocks only mode and clear the mempool to easily show the demo
```
./build/bin/bitcoind -datadir=<datadir2> -port=38333 -rpcport=38332 -rpcbind=0.0.0.0 -zmqpubrawtx=tcp://127.0.0.1:28342 -persistmempool=0 -blocksonly=1
````

```
cargo run -- --bitcoinrpc http://127.0.0.1:8332 --datadir <datadir1> --zmq tcp://127.0.0.1:28332
```

```
cargo run -- --bitcoinrpc http://127.0.0.1:38332 --datadir <datadir2> --zmq tcp://127.0.0.1:38332
```

Inspired by

https://github.com/benthecarman/nostr-tx-broadcast

https://gnusha.org/pi/bitcoindev/CAJBJmV932eeuiBzo_EMxJ1iU=Gave9=PC3U7seVoBXUFsu_GUA@mail.gmail.com/




