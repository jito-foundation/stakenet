//! An [`RpcSender`] that sends transactions straight to the leaders' TPU over QUIC.
//!
//! Every RPC method except `sendTransaction` passes through to the wrapped client, so an
//! `RpcClient` built from [`TpuSender`] is a drop-in replacement when the RPC node can serve
//! reads but not sends. It does what the node would do on `sendTransaction`: preflight
//! simulation (unless `skip_preflight`) and rebroadcast until the transaction lands or expires.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use base64::{prelude::BASE64_STANDARD, Engine};
use futures::StreamExt;
use log::*;
use serde_json::{json, Value};
use solana_client::{
    client_error::{ClientError, ClientErrorKind, Result as ClientResult},
    nonblocking::{
        pubsub_client::PubsubClient,
        rpc_client::RpcClient,
        tpu_client::{TpuClient, TpuSenderError},
    },
    rpc_client::RpcClientConfig,
    rpc_config::{RpcSendTransactionConfig, RpcSimulateTransactionConfig},
    rpc_custom_error::JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE,
    rpc_request::{RpcError, RpcRequest, RpcResponseErrorData},
    rpc_sender::{RpcSender, RpcTransportStats},
    tpu_client::TpuClientConfig,
};
use solana_quic_client::{QuicConfig, QuicConnectionManager, QuicPool};
use solana_sdk::{
    bs58, commitment_config::CommitmentConfig, signature::Signature,
    transaction::VersionedTransaction,
};
use solana_transaction_status::{TransactionBinaryEncoding, UiTransactionEncoding};
use tokio::time::{sleep, timeout};
use url::Url;

/// Same cadence as the RPC node's default `--rpc-send-retry-ms`
const REBROADCAST_INTERVAL: Duration = Duration::from_secs(2);
/// Comfortably past the ~60s blockhash lifetime; resending an expired transaction is harmless
const REBROADCAST_TTL: Duration = Duration::from_secs(90);
const SLOT_UPDATES_TIMEOUT: Duration = Duration::from_secs(10);
/// Max signatures per `getSignatureStatuses` call
const SIG_STATUS_BATCH_SIZE: usize = 256;

type QuicTpuClient = TpuClient<QuicPool, QuicConnectionManager, QuicConfig>;
/// signature -> (wire transaction, first sent at)
type PendingTransactions = Arc<Mutex<HashMap<Signature, (Vec<u8>, Instant)>>>;

pub struct TpuSender {
    rpc: Arc<RpcClient>,
    tpu: Arc<QuicTpuClient>,
    pending: PendingTransactions,
}

impl TpuSender {
    pub async fn new(rpc: Arc<RpcClient>, websocket_url: &str) -> Result<Self, TpuSenderError> {
        check_slot_updates(websocket_url).await?;
        let tpu = Arc::new(
            TpuClient::new(
                "stakenet-tpu",
                rpc.clone(),
                websocket_url,
                TpuClientConfig::default(),
            )
            .await?,
        );
        let pending = PendingTransactions::default();
        tokio::spawn(rebroadcast(rpc.clone(), tpu.clone(), pending.clone()));

        info!("Sending transactions over TPU websocket_url={websocket_url}");
        Ok(Self { rpc, tpu, pending })
    }

    async fn send_transaction(&self, params: Value) -> ClientResult<Value> {
        let config: RpcSendTransactionConfig =
            serde_json::from_value(params[1].clone()).unwrap_or_default();
        let encoded = params[0].as_str().unwrap_or_default();
        let wire = match config
            .encoding
            .unwrap_or(UiTransactionEncoding::Base58)
            .into_binary_encoding()
        {
            Some(TransactionBinaryEncoding::Base58) => {
                bs58::decode(encoded).into_vec().map_err(invalid)?
            }
            Some(TransactionBinaryEncoding::Base64) => {
                BASE64_STANDARD.decode(encoded).map_err(invalid)?
            }
            None => return Err(invalid("unsupported transaction encoding")),
        };
        let transaction: VersionedTransaction = bincode::deserialize(&wire).map_err(invalid)?;
        let signature = *transaction
            .signatures
            .first()
            .ok_or_else(|| invalid("transaction has no signatures"))?;

        if !config.skip_preflight {
            self.preflight(&transaction, &config).await?;
        }

        if !self.tpu.send_wire_transaction(wire.clone()).await {
            warn!("TPU send reached no leader, will rebroadcast signature={signature}");
        }
        self.pending
            .lock()
            .unwrap()
            .insert(signature, (wire, Instant::now()));

        Ok(json!(signature.to_string()))
    }

    /// Simulates like the RPC node's `sendTransaction` preflight, and fails with the same error.
    async fn preflight(
        &self,
        transaction: &VersionedTransaction,
        config: &RpcSendTransactionConfig,
    ) -> ClientResult<()> {
        let simulation = self
            .rpc
            .simulate_transaction_with_config(
                transaction,
                RpcSimulateTransactionConfig {
                    sig_verify: true,
                    commitment: Some(CommitmentConfig {
                        commitment: config.preflight_commitment.unwrap_or_default(),
                    }),
                    min_context_slot: config.min_context_slot,
                    ..RpcSimulateTransactionConfig::default()
                },
            )
            .await;

        match simulation {
            Ok(response) => match &response.value.err {
                None => Ok(()),
                Some(err) => Err(RpcError::RpcResponseError {
                    code: JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE,
                    message: format!("Transaction simulation failed: {err}"),
                    data: RpcResponseErrorData::SendTransactionPreflightFailure(
                        response.value.clone(),
                    ),
                }
                .into()),
            },
            // The node already can't send, so don't let a broken simulate block the TPU path too
            Err(e) => {
                warn!("TPU preflight simulation failed, sending without it error={e}");
                Ok(())
            }
        }
    }
}

#[async_trait]
impl RpcSender for TpuSender {
    async fn send(&self, request: RpcRequest, params: Value) -> ClientResult<Value> {
        match request {
            RpcRequest::SendTransaction => self.send_transaction(params).await,
            _ => self.rpc.send(request, params).await,
        }
    }

    fn get_transport_stats(&self) -> RpcTransportStats {
        self.rpc.get_transport_stats()
    }

    fn url(&self) -> String {
        self.rpc.url()
    }
}

/// An `RpcClient` that sends transactions over TPU and serves everything else from `json_rpc_url`.
pub async fn new_tpu_rpc_client(
    json_rpc_url: String,
    websocket_url: &str,
    rpc_timeout: Duration,
    commitment: CommitmentConfig,
) -> Result<RpcClient, TpuSenderError> {
    let rpc = Arc::new(RpcClient::new_with_timeout_and_commitment(
        json_rpc_url,
        rpc_timeout,
        commitment,
    ));
    let sender = TpuSender::new(rpc, websocket_url).await?;

    Ok(RpcClient::new_sender(
        sender,
        RpcClientConfig::with_commitment(commitment),
    ))
}

/// Derives the websocket URL from an RPC URL the way the `solana` CLI does: http→ws, https→wss,
/// and an explicit port moves up by one (8899 → 8900).
pub fn derive_websocket_url(json_rpc_url: &str) -> Option<String> {
    let mut url = Url::parse(json_rpc_url).ok()?;
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme).ok()?;
    if let Some(port) = url.port() {
        url.set_port(Some(port.checked_add(1)?)).ok()?;
    }

    Some(url.into())
}

/// `TpuClient` tracks the current slot over `slotsUpdatesSubscribe`, and if that subscription
/// fails it keeps quietly sending to the leaders from startup. Fail loudly here instead.
async fn check_slot_updates(websocket_url: &str) -> Result<(), TpuSenderError> {
    let check = async {
        let pubsub = PubsubClient::new(websocket_url).await?;
        let (mut updates, unsubscribe) = pubsub.slot_updates_subscribe().await?;
        let received = updates.next().await.is_some();
        drop(updates);
        unsubscribe().await;
        pubsub.shutdown().await?;
        Ok::<_, TpuSenderError>(received)
    };

    match timeout(SLOT_UPDATES_TIMEOUT, check).await {
        Ok(Ok(true)) => Ok(()),
        Ok(Err(e)) => Err(e),
        _ => Err(TpuSenderError::Custom(format!(
            "No slotsUpdatesSubscribe notifications from {websocket_url}"
        ))),
    }
}

/// Resends pending transactions until `getSignatureStatuses` sees them or they expire, like the
/// RPC node's send-transaction-service.
async fn rebroadcast(rpc: Arc<RpcClient>, tpu: Arc<QuicTpuClient>, pending: PendingTransactions) {
    loop {
        sleep(REBROADCAST_INTERVAL).await;

        let signatures: Vec<Signature> = {
            let mut pending = pending.lock().unwrap();
            pending.retain(|_, (_, first_sent)| first_sent.elapsed() < REBROADCAST_TTL);
            pending.keys().copied().collect()
        };
        for batch in signatures.chunks(SIG_STATUS_BATCH_SIZE) {
            if let Ok(statuses) = rpc.get_signature_statuses(batch).await {
                let mut pending = pending.lock().unwrap();
                for (signature, status) in batch.iter().zip(statuses.value) {
                    if status.is_some() {
                        pending.remove(signature);
                    }
                }
            }
        }

        let wire_transactions: Vec<Vec<u8>> = pending
            .lock()
            .unwrap()
            .values()
            .map(|(wire, _)| wire.clone())
            .collect();
        if wire_transactions.is_empty() {
            continue;
        }
        if let Err(e) = tpu.try_send_wire_transaction_batch(wire_transactions).await {
            debug!("TPU rebroadcast failed error={e}");
        }
    }
}

fn invalid(err: impl std::fmt::Display) -> ClientError {
    ClientErrorKind::Custom(format!("TPU sendTransaction: {err}")).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_websocket_url_bumps_explicit_port() {
        assert_eq!(
            derive_websocket_url("http://127.0.0.1:8899").as_deref(),
            Some("ws://127.0.0.1:8900/")
        );
    }

    #[test]
    fn test_derive_websocket_url_keeps_path_and_query() {
        assert_eq!(
            derive_websocket_url("https://rpc.example.com/v1/token?api-key=abc").as_deref(),
            Some("wss://rpc.example.com/v1/token?api-key=abc")
        );
    }

    #[test]
    fn test_derive_websocket_url_rejects_invalid_url() {
        assert_eq!(derive_websocket_url("not a url"), None);
    }
}
