use std::{str::FromStr, sync::Arc};

use anchor_lang::AccountDeserialize;
use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Json,
};
use serde_derive::Deserialize;
use solana_program::pubkey::Pubkey;
use stakenet_sdk::utils::accounts::get_validator_history_address;
use tracing::warn;
use validator_history::ValidatorHistory;

use crate::{error::ApiError, ValidatorHistoryEntryResponse, ValidatorHistoryResponse};

use super::RouterState;

#[derive(Deserialize)]
pub(crate) struct EpochQuery {
    epoch: Option<u16>,
}

/// Retrieves the history of a specific validator, based on the provided vote account and optional epoch filter.
///
/// # Returns
/// - `Ok(Json(history))`: A JSON response containing the validator history information. If the epoch filter is provided, it only returns the history for the specified epoch.
///
/// # Example
/// This endpoint can be used to fetch the history of a validator's performance over time, either for a specific epoch or for all recorded epochs:
/// ```
/// GET /validator_history/{vote_account}?epoch=200
/// ```
/// This request retrieves the history for the specified vote account, filtered by epoch 200.
pub(crate) async fn get_all_validator_histories(
    State(state): State<Arc<RouterState>>,
    Path(vote_account): Path<String>,
    Query(epoch_query): Query<EpochQuery>,
) -> crate::Result<impl IntoResponse> {
    let vote_account = Pubkey::from_str(&vote_account)?;
    let history_account =
        get_validator_history_address(&vote_account, &state.validator_history_program_id);
    let account = state.rpc_client.get_account(&history_account).await?;
    let validator_history = ValidatorHistory::try_deserialize(&mut account.data.as_slice())
        .map_err(|e| {
            warn!("error deserializing ValidatorHistory: {:?}", e);
            ApiError::ValidatorHistoryError("Error parsing ValidatorHistory".to_string())
        })?;

    let history_entries: Vec<ValidatorHistoryEntryResponse> = match epoch_query.epoch {
        Some(epoch) => validator_history
            .history
            .arr
            .iter()
            .filter_map(|entry| {
                if epoch == entry.epoch {
                    Some(ValidatorHistoryEntryResponse::from_validator_history_entry(
                        entry,
                    ))
                } else {
                    None
                }
            })
            .collect(),
        None => validator_history
            .history
            .arr
            .iter()
            .map(ValidatorHistoryEntryResponse::from_validator_history_entry)
            .collect(),
    };

    let history =
        ValidatorHistoryResponse::from_validator_history(validator_history, history_entries);

    Ok(Json(history))
}
