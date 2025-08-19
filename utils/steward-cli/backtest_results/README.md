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
- **Comparison Score**: Uses `mev_ranking_score` as `score_for_backtest_comparison`
- **Date**: 2025-08-19

## Schema Notes

All files use the extended `ValidatorScoreResult` schema that includes:
- All original scoring components
- `mev_ranking_score`: 1.0 - (max_mev_commission / 10000.0) 
- `validator_age`: Consecutive epochs above voting threshold
- `score_for_backtest_comparison`: Consistent metric for comparing different strategies
- `vote_account`: Encoded as base58 string for readability

The `score_for_backtest_comparison` field enables meaningful comparisons across different scoring strategies, as the raw `score` field may have different interpretations depending on the strategy used.