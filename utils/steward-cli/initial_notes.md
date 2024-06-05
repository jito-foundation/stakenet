
# Initial Commands

## Create Config
```bash
cargo run init-config --authority-keypair-path ../../credentials/stakenet_test.json --stake-pool 3DuPtyTAKrxKfHkSPZ5fqCayMcGru1BarAKKTfGDeo2j \
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

# Initial Parameters

```txt
mev_commission_range: 10
epoch_credits_range: 30
commission_range: 30
mev_commission_bps_threshold: 1000
commission_threshold: 5
historical_commission_threshold: 50
scoring_delinquency_threshold_ratio: 0.85
instant_unstake_delinquency_threshold_ratio: 0.70
num_delegation_validators: 200
scoring_unstake_cap_bps: 750
instant_unstake_cap_bps: 1000
stake_deposit_unstake_cap_bps: 1000
compute_score_slot_range: 1000
instant_unstake_epoch_progress: 0.50
instant_unstake_inputs_epoch_progress: 0.50
num_epochs_between_scoring: 3
minimum_stake_lamports: 100_000_000_000
minimum_voting_epochs: 5
```