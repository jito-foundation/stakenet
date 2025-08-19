# Backtest Results

This directory contains backtest results for different scoring configurations.

## Files

### 01.json
- **Configuration**: Production scoring (baseline)
- **Ranking**: Sort by overall `score` field (product of all binary components * yield_score)
- **Description**: Current production behavior - binary filters multiply with continuous yield score
- **Formula**: `score = mev_commission_score * commission_score * historical_commission_score * blacklisted_score * superminority_score * delinquency_score * running_jito_score * yield_score * merkle_root_upload_authority_score`
- **Date**: 2025-08-19

## Schema Notes

All files use the extended `ValidatorScoreResult` schema that includes:
- All original scoring components
- `mev_ranking_score`: 1.0 - (max_mev_commission / 10000.0) 
- `validator_age`: Consecutive epochs above voting threshold

Even when using production scoring, these additional fields are calculated and stored for future comparison purposes.