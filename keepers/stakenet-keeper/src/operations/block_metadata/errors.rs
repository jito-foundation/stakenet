use solana_client::{client_error::ClientError, rpc_request::RpcError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BlockMetadataKeeperError {
    #[error("SolanaClientError error: {0}")]
    SolanaClientError(#[from] ClientError),
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
