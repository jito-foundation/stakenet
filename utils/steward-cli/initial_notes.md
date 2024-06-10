
# Accounts

**Authority** 
`aaaDerwdMyzNkoX1aSoTi3UtFe2W45vh5wCgQNhsjF8`

**Steward Config**   
`BF9n2VmQT7DLB8h8STmyghpnYV8pPRUj3DCe3gAWyT1S`

**Stake Pool**
`3DuPtyTAKrxKfHkSPZ5fqCayMcGru1BarAKKTfGDeo2j`

**Staker**
`Eo3dSDhQi11AJbsoK19i1kBRLzpBiFLkWBq7pdVkfucG`

**State**
`7iZSeAiJNpW86VtUceL3a49xkTShcwgG1UwaMuXssxXN`

# Initial Commands

## Create Config
```bash
cargo run init-config \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --stake-pool 3DuPtyTAKrxKfHkSPZ5fqCayMcGru1BarAKKTfGDeo2j \
  --mev-commission-range 10 \
  --epoch-credits-range 30 \
  --commission-range 30 \
  --mev-commission-bps-threshold 1000 \
  --commission-threshold 5 \
  --historical-commission-threshold 50 \
  --scoring-delinquency-threshold-ratio 0.85 \
  --instant-unstake-delinquency-threshold-ratio 0.70 \
  --num-delegation-validators 200 \
  --scoring-unstake-cap-bps 750 \
  --instant-unstake-cap-bps 1000 \
  --stake-deposit-unstake-cap-bps 1000 \
  --compute-score-slot-range 1000 \
  --instant-unstake-epoch-progress 0.50 \
  --instant-unstake-inputs-epoch-progress 0.50 \
  --num-epochs-between-scoring 3 \
  --minimum-stake-lamports 100_000_000_000 \
  --minimum-voting-epochs 5
```

## Update Config
```bash
cargo run update-config \
  --authority-keypair-path ../../credentials/stakenet_test.json \
  --steward-config BF9n2VmQT7DLB8h8STmyghpnYV8pPRUj3DCe3gAWyT1S \
  --compute-score-slot-range 50000 
```

## View Config
```bash
cargo run view-config --steward-config BF9n2VmQT7DLB8h8STmyghpnYV8pPRUj3DCe3gAWyT1S
```

## Create State
```bash
cargo run init-state --authority-keypair-path ../../credentials/stakenet_test.json --stake-pool 3DuPtyTAKrxKfHkSPZ5fqCayMcGru1BarAKKTfGDeo2j --steward-config BF9n2VmQT7DLB8h8STmyghpnYV8pPRUj3DCe3gAWyT1S
```

## View State
```bash
cargo run view-state --steward-config BF9n2VmQT7DLB8h8STmyghpnYV8pPRUj3DCe3gAWyT1S
```

## Remove Bad Validators
```bash
cargo run remove-bad-validators --steward-config BF9n2VmQT7DLB8h8STmyghpnYV8pPRUj3DCe3gAWyT1S --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Score
```bash
cargo run crank-compute-score --steward-config BF9n2VmQT7DLB8h8STmyghpnYV8pPRUj3DCe3gAWyT1S --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Compute Delegations
```bash
cargo run crank-compute-delegations --steward-config BF9n2VmQT7DLB8h8STmyghpnYV8pPRUj3DCe3gAWyT1S --payer-keypair-path ../../credentials/stakenet_test.json
```

## Crank Idle
```bash
cargo run crank-idle --steward-config BF9n2VmQT7DLB8h8STmyghpnYV8pPRUj3DCe3gAWyT1S --payer-keypair-path ../../credentials/stakenet_test.json
```


# Initial Parameters

```env
# Note - Do not use this .env when updating the parameters - this will update them all
MEV_COMMISSION_RANGE=10
EPOCH_CREDITS_RANGE=30
COMMISSION_RANGE=30
MEV_COMMISSION_BPS_THRESHOLD=1000
COMMISSION_THRESHOLD=5
HISTORICAL_COMMISSION_THRESHOLD=50
SCORING_DELINQUENCY_THRESHOLD_RATIO=0.85
INSTANT_UNSTAKE_DELINQUENCY_THRESHOLD_RATIO=0.70
NUM_DELEGATION_VALIDATORS=200
SCORING_UNSTAKE_CAP_BPS=750
INSTANT_UNSTAKE_CAP_BPS=1000
STAKE_DEPOSIT_UNSTAKE_CAP_BPS=1000
COMPUTE_SCORE_SLOT_RANGE=1000
INSTANT_UNSTAKE_EPOCH_PROGRESS=0.50
INSTANT_UNSTAKE_INPUTS_EPOCH_PROGRESS=0.50
NUM_EPOCHS_BETWEEN_SCORING=3
MINIMUM_STAKE_LAMPORTS=100000000000
MINIMUM_VOTING_EPOCHS=5
```