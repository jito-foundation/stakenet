# Directed Staking CLI

## Accounts

| Account        | Address                                      |
| -------------- | -------------------------------------------- |
| Program        | Stewardf95sJbmtcZsyagb2dg4Mo8eVQho8gpECvLx8  |
| Steward Config | jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv  |
| Steward State  | 9BAmGVLGxzqct6bkgjWmKSv3BFB6iKYXNBQp8GWG1LDY |
| Authority      | 9eZbWiHsPRsxLSiHxzg2pkXsAuQMwAjQrda7C7e21Fw6 |

## CLI Commands

### View Config

```bash
cargo r -p directed-staking-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    view-config \
    --config DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi \
```

### View Directed Stake Whitelist

```bash
cargo r -p directed-staking-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    view-directed-stake-whitelist \
    --steward-config DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi \
```

### View Directed Stake Meta

```bash
cargo r -p directed-staking-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    view-directed-stake-meta \
    --steward-config DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi \
```

### View Directed Stake Tickets

```bash
cargo r -p directed-staking-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    view-directed-stake-tickets
```

### View Directed Stake Ticket

```bash
cargo r -p directed-staking-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    view-directed-stake-ticket \
    --ticket-signer BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA
```

### Update Stake Meta Upload authority

```bash
cargo r -p steward-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    update-authority \
    directed-stake-meta-upload \
    --steward-config DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi \
    --new-authority BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA \
    --authority-keypair-path ~/.config/solana/id.json
```

### Update Stake Whitelist authority

```bash
cargo r -p steward-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    --program-id 3YeBnUPN2ZW8MBVb8695Hdffu8jBpRjm6BUazRexHDTg \
    update-authority \
     directed-stake-whitelist  \
    --steward-config DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi \
    --new-authority BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA \
    --authority-keypair-path ~/.config/solana/id.json
```

### Initialize Whitelist

```bash
cargo r -p directed-staking-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    init-directed-stake-whitelist \
    --steward-config DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi \
    --authority-keypair-path ~/.config/solana/id.json

# Initializing DirectedStakeWhitelist...
#   Authority:
#   Steward Config: DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi
#   DirectedStakeWhitelist PDA: 83U6qSYdAuEZJiZYzkg4Rb7XyRiM4rpa2fjTE2ieA2X
# ✅ DirectedStakeWhitelist initialized successfully!
#   Transaction signature: 3FDMPL4kJJPneNgo2CHikLxsBrSGu9sSeuf2qin9CYwmsaJRAYepr5ftMt2KgAnBaUQ51r3X2iRoahNavzPXQbZE
#   DirectedStakeWhitelist account: 83U6qSYdAuEZJiZYzkg4Rb7XyRiM4rpa2fjTE2ieA2X
```

### Realloc Whitelist

```bash
cargo r -p directed-staking-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    realloc-directed-stake-whitelist \
    --steward-config DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi \
    --authority-keypair-path ~/.config/solana/id.json
```

### Initialize Stake Meta

```bash
cargo r -p directed-staking-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    init-directed-stake-meta \
    --steward-config DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi \
    --authority-keypair-path ~/.config/solana/id.json

# Initializing DirectedStakeMeta...
#   Authority:
#   Steward Config: DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi
#   DirectedStakeMeta PDA: HK1WwbCnpefRfiZMTacHNMhLyU621uonSPCyCpB6mdp
# ✅ DirectedStakeMeta initialized successfully!
#   Transaction signature: 2LXz9D6B5o3rs4bkQxhUju4bQZLXrmBni2AkawJCoXKv8VDR7H6rYxwQYeAjCViw2NNcsY7wdU2s3p41LBjjsgyn
#   DirectedStakeMeta account: HK1WwbCnpefRfiZMTacHNMhLyU621uonSPCyCpB6mdp
```

### Realloc Stake Meta

```bash
cargo r -p directed-staking-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    realloc-directed-stake-meta \
    --steward-config DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi \
    --authority-keypair-path ~/.config/solana/id.json
```

### Add to Directed stake whitelist

#### Validator

```bash
cargo r -p directed-staking-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    add-to-directed-stake-whitelist \
    --steward-config DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi \
    --authority-keypair-path ~/.config/solana/id.json \
    --record-type "validator" \
    --record BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA
```

#### User

```bash
cargo r -p directed-staking-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    add-to-directed-stake-whitelist \
    --steward-config DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi \
    --authority-keypair-path ~/.config/solana/id.json \
    --record-type "user" \
    --record BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA
```

#### Protocol

```bash
cargo r -p directed-staking-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    add-to-directed-stake-whitelist \
    --steward-config DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi \
    --authority-keypair-path ~/.config/solana/id.json \
    --record-type "protocol" \
    --record BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA
```

### Initialize Stake Ticket

```bash
cargo r -p directed-staking-cli -- \
    --json-rpc-url http://127.0.0.1:8899 \
    init-directed-stake-ticket \
    --steward-config DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi \
    --authority-keypair-path ~/.config/solana/id.json \
    --ticket-update-authority BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA

# Initializing DirectedStakeTicket...
#   Authority: BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA
#   Steward Config: DLLuDbFmQscieKjW88MgtUsYcSRNCGvupEwEQZN36wXi
#   Ticket Update Authority: BBBATax9kikSHQp8UTcyQL3tfU3BmQD9yid5qhC7QEAA
#   Ticket Holder Is Protocol: false
#   DirectedStakeTicket PDA: 4j6nu2W19qimz61VJUHGVQ31fa5skaT1bfSRVUWNVnLJ
# ✅ DirectedStakeTicket initialized successfully!
#   Transaction signature: 39iHv6nWkmVremYN1s4EHYxREwattZMjQFSb19dZ5YrC8JN85Tr4e1A5TF5WDq5zVaEMwasmrNwqueLSDBEsUvCd
#   DirectedStakeTicket account: 4j6nu2W19qimz61VJUHGVQ31fa5skaT1bfSRVUWNVnLJ
```
