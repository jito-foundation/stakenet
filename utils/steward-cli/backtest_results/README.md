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
- **Validator Age Definition**: Number of consecutive epochs (going backwards from current) where validator had vote credits above the voting threshold (0.99)
- **Comparison Score**: Uses `mev_ranking_score` as `score_for_backtest_comparison`
- **Date**: 2025-08-19

## Diff Analysis

For computing diffs between strategies, only two fields are needed:
- `vote_account`: Base58 string identifier for each validator
- `score_for_backtest_comparison`: Normalized comparison score across strategies

This enables ranking comparisons, position changes, and score distribution analysis between different backtest results.
