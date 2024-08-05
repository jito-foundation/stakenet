use log::*;
use solana_client::client_error::ClientError;
use solana_client::rpc_response::RpcSimulateTransactionResult;

use thiserror::Error as ThisError;
use tokio::task::JoinError;

#[derive(ThisError, Debug)]
pub enum JitoTransactionError {
    #[error(transparent)]
    ClientError(#[from] ClientError),
    #[error(transparent)]
    TransactionExecutionError(#[from] JitoTransactionExecutionError),
    #[error(transparent)]
    MultipleAccountsError(#[from] JitoMultipleAccountsError),
    #[error("Custom: {0}")]
    Custom(String),
}

pub type Error = Box<dyn std::error::Error>;
#[derive(ThisError, Debug, Clone)]
pub enum JitoTransactionExecutionError {
    #[error("RPC Client error: {0:?}")]
    ClientError(String),
    #[error("RPC Client error: {0:?}")]
    TransactionClientError(String, Vec<Result<(), JitoSendTransactionError>>),
}

#[derive(ThisError, Debug)]
pub enum JitoMultipleAccountsError {
    #[error(transparent)]
    ClientError(#[from] ClientError),
    #[error(transparent)]
    JoinError(#[from] JoinError),
}

#[derive(ThisError, Clone, Debug)]
pub enum JitoSendTransactionError {
    #[error("Exceeded retries")]
    ExceededRetries,
    // Stores ClientError.to_string(), since ClientError does not impl Clone, and we want to track both
    // io/reqwest errors as well as transaction errors
    #[error("Transaction error: {0}")]
    TransactionError(String),

    #[error("Verbose RPC Error")]
    RpcSimulateTransactionResult(RpcSimulateTransactionResult),
}
