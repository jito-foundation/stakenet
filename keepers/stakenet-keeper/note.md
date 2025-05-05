# Test

```bash
RUST_LOG=info cargo run -- --json-rpc-url 'https://jitolab-develope-4572.testnet.rpcpool.com/9f114e59-b01b-456b-9ca4-36ded3aa2403' --keypair /Users/christiankrueger/.config/solana/projects/dev.json --priority-fee-oracle-authority-keypair /Users/christiankrueger/.config/solana/projects/steward.json --oracle-authority-keypair /Users/christiankrueger/.config/solana/projects/steward.json --tip-distribution-program-id F2Zu7QZiTYUhPd7u9ukRVwxh7B71oA3NMJcHuCHc29P2 --steward-config 5pZmpk3ktweGZW9xFknpEHhQoWeAKTzSGwnCUyVdiye --priority-fee-distribution-program-id 9yw8YAKz16nFmA9EvHzKyVCYErHAJ6ZKtmK6adDBvmuU --validator-history-program-id HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa --cluster testnet  --run-block-metadata --block-metadata-interval 60 --sqlite-path /Users/christiankrueger/.config/solana/projects/sql/db --run-priority-fee-commission
```

```bash
cargo run -- -j 'https://jitolab-develope-4572.testnet.rpcpool.com/9f114e59-b01b-456b-9ca4-36ded3aa2403' history 13sfDC74Rqz6i2cMWjY5wqyRaNdpdpRd75pGLYPzqs44
```
