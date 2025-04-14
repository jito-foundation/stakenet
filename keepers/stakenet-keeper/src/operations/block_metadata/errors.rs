use solana_client::{client_error::ClientError, rpc_request::RpcError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BlockMetadataKeeperError {
    #[error("SoloanaClientError error: {0}")]
    SoloanaClientError(#[from] ClientError),
    #[error(transparent)]
    RpcError(#[from] RpcError),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    SqliteError(#[from] rusqlite::Error),
    #[error("No leader schedule for epoch found")]
    ErrorGettingLeaderSchedule,
    #[error("Block was skipped")]
    SkippedBlock,
}
