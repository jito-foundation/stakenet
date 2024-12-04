use std::{str::FromStr, sync::Arc};

use anchor_lang::AccountDeserialize;
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use solana_program::pubkey::Pubkey;
use stakenet_sdk::utils::accounts::get_validator_history_address;
use tracing::warn;
use validator_history::ValidatorHistory;

use crate::{error::ApiError, ValidatorHistoryEntryResponse, ValidatorHistoryResponse};

use super::RouterState;

/// Retrieves the latest historical entry for a specific validator based on the provided vote account.
///
/// # Returns
/// - `Ok(Json(history))`: A JSON response containing the most recent entry from the validator's history, wrapped in a [`ValidatorHistoryResponse`].
///
/// # Example
/// This method can be used to query the latest performance record for a validator:
/// ```
/// GET /validator_history/{vote_account}/latest
/// ```
/// This will return the most recent history entry for the validator associated with the given vote account.
pub(crate) async fn get_latest_validator_history(
    State(state): State<Arc<RouterState>>,
    Path(vote_account): Path<String>,
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

    match validator_history.history.last() {
        Some(entry) => {
            let history_entry = ValidatorHistoryEntryResponse::from_validator_history_entry(entry);
            let history = ValidatorHistoryResponse::from_validator_history(
                validator_history,
                vec![history_entry],
            );
            Ok(Json(history))
        }
        None => Err(ApiError::ValidatorHistoryNotFound(vote_account.to_string())),
    }
}
