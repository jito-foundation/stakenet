# Backtest Results

This directory contains backtest results for different scoring configurations.

## Backtest Configuration

- **Epoch Selection**: Only scores at rebalancing epochs (every 10 epochs) to match operational behavior
- **Default Lookback**: 10 rebalancing periods (100 epochs total)
- **Vote Account Format**: Base58 strings for human readability

## Files

### 01.json
- **Configuration**: Production scoring (baseline)
- **Ranking**: Sort by overall `score` field (product of all binary components * yield_score)
- **Description**: Current production behavior - binary filters multiply with continuous yield score
- **Formula**: `score = mev_commission_score * commission_score * historical_commission_score * blacklisted_score * superminority_score * delinquency_score * running_jito_score * yield_score * merkle_root_upload_authority_score`
- **Comparison Score**: Uses `score` field as `score_for_backtest_comparison`
- **Date**: 2025-08-19

### 02.json
- **Configuration**: MEV commission + validator age ranking
- **Ranking**: Primary by `mev_ranking_score` (1.0 - max_mev_commission/10000), tiebreaker by `validator_age`
- **Description**: Alternative strategy prioritizing low MEV commission with validator age as tiebreaker
- **Binary Filters**: Same as production (all must pass for validator to be eligible)
- **Validator Age Definition**: Total number of epochs with non-null vote credits (gaps allowed, no threshold requirement)
- **Comparison Score**: Uses `mev_ranking_score` as `score_for_backtest_comparison`
- **Date**: 2025-08-19

## Diff Analysis

For computing diffs between strategies, only two fields are needed:
- `vote_account`: Base58 string identifier for each validator
- `score_for_backtest_comparison`: Normalized comparison score across strategies

This enables ranking comparisons, position changes, and score distribution analysis between different backtest results.

## Steward Configuration Reference

```
Config: jitoVjT9jRUyeXHzvCwzPgHj7yWNRhLcUoXtes4wtjv
Stake Pool: Jito4APyf642JPZPx3hGc6WWJ8zPKtRbRs4P815Awbb

Parameters:
- Commission Range: 30 epochs
- MEV Commission Range: 10 epochs  
- Epoch Credits Range: 30 epochs
- MEV Commission BPS Threshold: 1000 (10%)
- Scoring Delinquency Threshold Ratio: 0.85 (85% vote credits required)
- Instant Unstake Delinquency Threshold Ratio: 0.7
- Commission Threshold: 5%
- Historical Commission Threshold: 50%
- Number of Delegation Validators: 200
- Scoring Unstake Cap BPS: 750
- Instant Unstake Cap BPS: 1000
- Stake Deposit Unstake Cap BPS: 1000
- Compute Score Slot Range: 10000
- Instant Unstake Epoch Progress: 0.9
- Instant Unstake Inputs Epoch Progress: 0.5
- Number of Epochs Between Scoring: 10
- Minimum Stake Lamports: 5000000000000
- Minimum Voting Epochs: 5
- Blacklisted Validators: 115
```

### Key Binary Filters (any failure = score of 0)
- **Delinquency**: Any epoch < 85% vote credits in 30-epoch window
- **MEV Commission**: Max MEV commission > 10% in 10-epoch window
- **Commission**: Max commission > 5% in 30-epoch window  
- **Historical Commission**: Max commission > 50% in all history
- **Running Jito**: Must have MEV commission set in last 10 epochs
- **Minimum Voting**: Must have voted in at least 5 epochs
