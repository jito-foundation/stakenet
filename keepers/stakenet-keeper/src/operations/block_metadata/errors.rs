use solana_client::{
    client_error::{ClientError, ClientErrorKind},
    rpc_request::RpcError,
};
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
    #[error("Vote key not found for identity {0}")]
    MissingVoteKey(String),
    #[error("Slot {0} not found. SlotHistory not up to date or slot in future")]
    SlotInFuture(u64),
    #[error("Other Error {0}")]
    OtherError(String),
}

impl BlockMetadataKeeperError {
    /// Whether retrying this fetch could ever succeed.
    pub fn is_permanent(&self) -> bool {
        match self {
            Self::SolanaClientError(err) => {
                matches!(err.kind(), ClientErrorKind::SerdeJson(_))
            }
            _ => false,
        }
    }
}
