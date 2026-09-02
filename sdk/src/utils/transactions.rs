use std::collections::HashSet;
use std::mem::size_of;
use std::vec;
use std::{collections::HashMap, sync::Arc, time::Duration};

use log::*;
use solana_client::rpc_config::{RpcSendTransactionConfig, RpcSimulateTransactionConfig};
use solana_client::rpc_response::{
    Response, RpcResult, RpcSimulateTransactionResult, RpcVoteAccountInfo,
};
use solana_client::{client_error::ClientError, nonblocking::rpc_client::RpcClient};
use solana_metrics::datapoint_error;
use solana_program::hash::Hash;
use solana_sdk::bs58;
use solana_sdk::commitment_config::CommitmentLevel;
use solana_sdk::compute_budget::{ComputeBudgetInstruction, ID as COMPUTE_BUDGET_ID};
use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::transaction::TransactionError;
use solana_sdk::{
    account::Account, commitment_config::CommitmentConfig, instruction::AccountMeta,
    instruction::Instruction, instruction::InstructionError, packet::Packet, pubkey::Pubkey,
    signature::Keypair, signature::Signature, signer::Signer, transaction::Transaction,
};
use solana_transaction_status::TransactionStatus;
use tokio::task;
use tokio::time::sleep;

use crate::models::errors::{
    JitoMultipleAccountsError, JitoSendTransactionError, JitoTransactionExecutionError,
};
use crate::models::submit_stats::SubmitStats;

use std::future::Future;

pub const DEFAULT_COMPUTE_LIMIT: u64 = 200_000;

/// Max compute units a single transaction may request.
pub const MAX_COMPUTE_LIMIT: u32 = 1_400_000;

/// True if this failure was the runtime rejecting the transaction for exhausting its
/// compute budget, whether reported by preflight simulation or by a landed transaction.
pub fn is_compute_budget_exceeded(error: &JitoSendTransactionError) -> bool {
    match error {
        JitoSendTransactionError::RpcSimulateTransactionResult(result) => matches!(
            result.err,
            Some(TransactionError::InstructionError(
                _,
                InstructionError::ComputationalBudgetExceeded
            ))
        ),
        JitoSendTransactionError::TransactionError(message) => {
            message.contains("ComputationalBudgetExceeded")
        }
        JitoSendTransactionError::ExceededRetries => false,
    }
}

/// Returns `instructions` with its compute unit limit set to `limit`, replacing an
/// existing `SetComputeUnitLimit` or prepending one if absent.
pub fn raise_compute_limit(instructions: &[Instruction], limit: u32) -> Vec<Instruction> {
    // SetComputeUnitLimit is discriminant 2 in the ComputeBudget instruction enum.
    const SET_COMPUTE_UNIT_LIMIT: u8 = 2;

    let mut replaced = false;
    let mut result: Vec<Instruction> = instructions
        .iter()
        .map(|ix| {
            if ix.program_id == COMPUTE_BUDGET_ID
                && ix.data.first() == Some(&SET_COMPUTE_UNIT_LIMIT)
            {
                replaced = true;
                ComputeBudgetInstruction::set_compute_unit_limit(limit)
            } else {
                ix.clone()
            }
        })
        .collect();

    if !replaced {
        result.insert(0, ComputeBudgetInstruction::set_compute_unit_limit(limit));
    }

    result
}

pub async fn retry<F, Fut, T, E>(mut f: F, retries: usize) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let mut attempts = 0;
    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                attempts += 1;
                if attempts > retries {
                    return Err(e);
                }
            }
        }
    }
}

pub async fn get_multiple_accounts_batched(
    accounts: &[Pubkey],
    rpc_client: &Arc<RpcClient>,
) -> Result<Vec<Option<Account>>, JitoMultipleAccountsError> {
    let tasks = accounts.chunks(100).map(|chunk| {
        let client = Arc::clone(rpc_client);
        let chunk = chunk.to_owned();
        task::spawn(
            async move { get_multiple_accounts_with_retry(&client, chunk.as_slice()).await },
        )
    });

    let mut accounts_result = Vec::new();
    for result in futures::future::join_all(tasks).await.into_iter() {
        match result {
            Ok(Ok(accounts)) => accounts_result.extend(accounts),
            Ok(Err(e)) => {
                return Err(JitoMultipleAccountsError::ClientError(e));
            }
            Err(e) => return Err(JitoMultipleAccountsError::JoinError(e)),
        }
    }
    Ok(accounts_result)
}

async fn simulate_instruction(
    client: &RpcClient,
    instruction: &Instruction,
    signer: &Keypair,
    priority_fee_in_microlamports: u64,
    max_cu_per_tx: u32,
) -> Result<Response<RpcSimulateTransactionResult>, ClientError> {
    let test_tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(max_cu_per_tx),
            ComputeBudgetInstruction::set_compute_unit_price(priority_fee_in_microlamports),
            instruction.to_owned(),
        ],
        Some(&signer.pubkey()),
        &[signer],
        Hash::default(),
    );

    client
        .simulate_transaction_with_config(
            &test_tx,
            RpcSimulateTransactionConfig {
                sig_verify: false,
                replace_recent_blockhash: true,
                ..RpcSimulateTransactionConfig::default()
            },
        )
        .await
}

async fn simulate_instruction_with_retry(
    client: &RpcClient,
    instruction: &Instruction,
    signer: &Keypair,
    priority_fee_in_microlamports: u64,
    max_cu_per_tx: u32,
) -> Result<Response<RpcSimulateTransactionResult>, ClientError> {
    for _ in 0..5 {
        match simulate_instruction(
            client,
            instruction,
            signer,
            priority_fee_in_microlamports,
            max_cu_per_tx,
        )
        .await
        {
            Ok(response) => match response.value.err {
                Some(e) => {
                    if e == TransactionError::BlockhashNotFound {
                        sleep(Duration::from_secs(3)).await;
                    } else {
                        return Err(e.into());
                    }
                }
                None => return Ok(response),
            },
            Err(e) => return Err(e),
        }
    }

    simulate_instruction(
        client,
        instruction,
        signer,
        priority_fee_in_microlamports,
        max_cu_per_tx,
    )
    .await
}

async fn get_latest_blockhash_with_retry(client: &RpcClient) -> Result<Hash, ClientError> {
    for _ in 1..4 {
        let result = client
            .get_latest_blockhash_with_commitment(CommitmentConfig::finalized())
            .await;
        if result.is_ok() {
            return Ok(result?.0);
        }
    }
    Ok(client
        .get_latest_blockhash_with_commitment(CommitmentConfig::finalized())
        .await?
        .0)
}

pub async fn get_multiple_accounts_with_retry(
    client: &RpcClient,
    pubkeys: &[Pubkey],
) -> Result<Vec<Option<Account>>, ClientError> {
    for _ in 1..4 {
        let result = client.get_multiple_accounts(pubkeys).await;
        if result.is_ok() {
            return result;
        }
    }
    client.get_multiple_accounts(pubkeys).await
}

pub async fn get_vote_accounts_with_retry(
    client: &RpcClient,
    min_vote_epochs: usize,
    commitment: Option<CommitmentConfig>,
) -> Result<Vec<RpcVoteAccountInfo>, ClientError> {
    for _ in 1..4 {
        let result = client
            .get_vote_accounts_with_commitment(commitment.unwrap_or(CommitmentConfig::finalized()))
            .await;
        if let Ok(response) = result {
            return Ok(response
                .current
                .into_iter()
                .chain(response.delinquent.into_iter())
                .filter(|vote_account| vote_account.epoch_credits.len() >= min_vote_epochs)
                .collect::<Vec<_>>());
        }
    }
    let result = client
        .get_vote_accounts_with_commitment(commitment.unwrap_or(CommitmentConfig::finalized()))
        .await;
    match result {
        Ok(response) => Ok(response
            .current
            .into_iter()
            .chain(response.delinquent.into_iter())
            .filter(|vote_account| vote_account.epoch_credits.len() >= min_vote_epochs)
            .collect::<Vec<_>>()),
        Err(e) => Err(e),
    }
}

pub async fn get_signature_statuses_with_retry(
    client: &RpcClient,
    signatures: &[Signature],
) -> RpcResult<Vec<Option<TransactionStatus>>> {
    for _ in 1..4 {
        if let Ok(result) = client.get_signature_statuses_with_history(signatures).await {
            return Ok(result);
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
    client.get_signature_statuses_with_history(signatures).await
}

async fn find_ix_per_tx(
    client: &Arc<RpcClient>,
    instruction: &Instruction,
    signer: &Arc<Keypair>,
    priority_fee_in_microlamports: u64,
    max_cu_per_tx: u32,
) -> Result<usize, ClientError> {
    let blockhash = get_latest_blockhash_with_retry(client).await?;
    let test_tx = Transaction::new_signed_with_payer(
        &[instruction.to_owned()],
        Some(&signer.pubkey()),
        &[signer],
        blockhash,
    );

    let response = simulate_instruction_with_retry(
        client,
        instruction,
        signer,
        priority_fee_in_microlamports,
        max_cu_per_tx,
    )
    .await?;
    if let Some(err) = response.value.clone().err {
        error!(
            "Instruction simulation failed max_cu_per_tx={} response={:?}",
            max_cu_per_tx, response.value
        );

        datapoint_error!(
            "simulation-error",
            ("error", err.to_string(), String),
            ("instruction", format!("{:?}", instruction), String)
        );

        return Err(err.into()); // Return the error immediately, stopping further execution
    }
    let compute = response
        .value
        .units_consumed
        .unwrap_or(DEFAULT_COMPUTE_LIMIT);

    let serialized_size = Packet::from_data(None, &test_tx).unwrap().meta().size;

    // additional size per ix
    let size_per_ix =
        instruction.accounts.len() * size_of::<AccountMeta>() + instruction.data.len();
    let size_max = (PACKET_DATA_SIZE - serialized_size + size_per_ix) / size_per_ix;

    let compute_max = max_cu_per_tx as usize / compute as usize;

    let size = size_max.min(compute_max);

    Ok(size)
}

async fn parallel_confirm_transactions(
    client: &RpcClient,
    submitted_signatures: HashSet<Signature>,
) -> HashSet<Signature> {
    // Confirms TXs in batches of 256 (max allowed by RPC method). Returns confirmed signatures
    const SIG_STATUS_BATCH_SIZE: usize = 256;
    let num_transactions_submitted = submitted_signatures.len();
    let signatures_to_confirm = submitted_signatures.into_iter().collect::<Vec<_>>();

    // Imperfect logic here: if a transaction is slow to confirm on first submission, and it can only be called once succesfully,
    // it will be resubmitted and fail. Ideally on the next loop it will not be included in the instructions list
    let confirmation_futures: Vec<_> = signatures_to_confirm
        .chunks(SIG_STATUS_BATCH_SIZE)
        .map(|sig_batch| async move {
            match get_signature_statuses_with_retry(client, sig_batch).await {
                Ok(sig_batch_response) => sig_batch_response
                    .value
                    .iter()
                    .enumerate()
                    .map(|(i, sig_status)| (sig_batch[i], sig_status.clone()))
                    .collect::<Vec<_>>(),
                Err(e) => {
                    warn!("Failed to get signature statuses: {e:?}");
                    vec![]
                }
            }
        })
        .collect();

    let results = futures::future::join_all(confirmation_futures).await;

    let mut confirmed_signatures: HashSet<Signature> = HashSet::new();

    for result_batch in results.iter() {
        for (sig, result) in result_batch {
            if let Some(status) = result {
                if status.satisfies_commitment(CommitmentConfig::confirmed())
                    && status.err.is_none()
                {
                    confirmed_signatures.insert(*sig);
                }
            }
        }
    }

    info!(
        "Confirmed transactions submitted={} confirmed={}",
        num_transactions_submitted,
        confirmed_signatures.len()
    );
    confirmed_signatures
}

fn sign_txs(
    transactions: &[&[Instruction]],
    signer: &Arc<Keypair>,
    blockhash: Hash,
) -> Vec<Transaction> {
    transactions
        .iter()
        .map(|instructions| {
            Transaction::new_signed_with_payer(
                instructions,
                Some(&signer.pubkey()),
                &[signer.as_ref()],
                blockhash,
            )
        })
        .collect()
}

/// Batch size for parallel submission - keeps blockhash fresh between batches
const SURFPOOL_BATCH_SIZE: usize = 100;

/// Fast transaction submission optimized for local surfpool node.
#[allow(unused)]
pub async fn _parallel_execute_transactions_surfpool(
    client: &Arc<RpcClient>,
    transactions: &[&[Instruction]],
    signer: &Arc<Keypair>,
    retry_count: u16,
    _confirmation_time: u64,
) -> Result<Vec<Result<(), JitoSendTransactionError>>, JitoTransactionExecutionError> {
    if transactions.is_empty() {
        return Ok(vec![]);
    }

    let mut results = vec![Err(JitoSendTransactionError::ExceededRetries); transactions.len()];

    let config = RpcSendTransactionConfig {
        skip_preflight: true,
        preflight_commitment: Some(CommitmentLevel::Processed),
        ..Default::default()
    };

    // Process in batches to keep blockhash fresh
    for (batch_start, batch) in transactions.chunks(SURFPOOL_BATCH_SIZE).enumerate() {
        let batch_offset = batch_start * SURFPOOL_BATCH_SIZE;
        let mut retries = 0;

        while retries < retry_count {
            // Fresh blockhash for each batch attempt
            let blockhash = get_latest_blockhash_with_retry(client)
                .await
                .map_err(|e| JitoTransactionExecutionError::ClientError(e.to_string()))?;

            // Only sign/submit transactions in this batch that haven't succeeded
            let pending: Vec<(usize, Transaction)> = batch
                .iter()
                .enumerate()
                .filter_map(|(i, ixs)| {
                    let global_idx = batch_offset + i;
                    if matches!(
                        results[global_idx],
                        Err(JitoSendTransactionError::ExceededRetries)
                    ) {
                        let tx = Transaction::new_signed_with_payer(
                            ixs,
                            Some(&signer.pubkey()),
                            &[signer.as_ref()],
                            blockhash,
                        );
                        Some((global_idx, tx))
                    } else {
                        None
                    }
                })
                .collect();

            if pending.is_empty() {
                break;
            }

            // Submit batch in parallel
            let futures: Vec<_> = pending
                .iter()
                .map(|(idx, tx)| {
                    let client = client.clone();
                    let idx = *idx;
                    let tx = tx.clone();
                    async move { (idx, client.send_transaction_with_config(&tx, config).await) }
                })
                .collect();

            let send_results = futures::future::join_all(futures).await;

            let mut needs_retry = false;

            for (idx, result) in send_results {
                match result {
                    Ok(_) => {
                        results[idx] = Ok(());
                    }
                    Err(e) => {
                        if let Some(tx_err) = e.get_transaction_error() {
                            match tx_err {
                                TransactionError::AlreadyProcessed => {
                                    results[idx] = Ok(());
                                }
                                TransactionError::BlockhashNotFound => {
                                    // Will retry with fresh blockhash
                                    needs_retry = true;
                                }
                                _ => {
                                    results[idx] = Err(JitoSendTransactionError::TransactionError(
                                        format!("TX Error: {tx_err:?}"),
                                    ));
                                }
                            }
                        } else {
                            results[idx] =
                                Err(JitoSendTransactionError::TransactionError(e.to_string()));
                        }
                    }
                }
            }

            if needs_retry {
                retries += 1;
            } else {
                break;
            }
        }
    }

    Ok(results)
}
pub async fn parallel_execute_transactions(
    client: &Arc<RpcClient>,
    transactions: &[&[Instruction]],
    signer: &Arc<Keypair>,
    retry_count: u16,
    confirmation_time: u64,
) -> Result<Vec<Result<(), JitoSendTransactionError>>, JitoTransactionExecutionError> {
    let mut results = vec![Err(JitoSendTransactionError::ExceededRetries); transactions.len()];
    let mut retries = 0;

    if transactions.is_empty() {
        return Ok(results);
    }

    let blockhash = get_latest_blockhash_with_retry(client)
        .await
        .map_err(|e| JitoTransactionExecutionError::ClientError(e.to_string()))?;
    let mut signed_txs = sign_txs(transactions, signer, blockhash);

    while retries < retry_count {
        let mut submitted_signatures = HashMap::new();
        let mut is_blockhash_not_found = false;

        for (idx, tx) in signed_txs.iter().enumerate() {
            if matches!(
                results[idx],
                Ok(_) | Err(JitoSendTransactionError::RpcSimulateTransactionResult(_))
            ) {
                continue; // Skip transactions that have already been confirmed
            }

            if idx % 20 == 0 {
                // Need to avoid spamming the rpc or lots of transactions will get dropped
                sleep(Duration::from_secs(1)).await;
            }

            // Future optimization: submit these in parallel batches and refresh blockhash for every batch
            match client.send_transaction(tx).await {
                Ok(signature) => {
                    debug!("Submitted transaction signature={signature}");
                    submitted_signatures.insert(signature, idx);
                }
                Err(e) => {
                    debug!("Transaction error: {e:?}");
                    match e.get_transaction_error() {
                        Some(TransactionError::BlockhashNotFound) => {
                            debug!("Blockhash not found, will retry");
                            is_blockhash_not_found = true;
                        }
                        Some(TransactionError::AlreadyProcessed) => {
                            debug!(
                                "Transaction already processed signature={}",
                                tx.signatures[0]
                            );
                            submitted_signatures.insert(tx.signatures[0], idx);
                        }
                        Some(_) => {
                            match e.kind {
                                solana_client::client_error::ClientErrorKind::Io(e) => {
                                                                results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                                                    "TX - Io Error: {e:?}"
                                                                )))
                                                            }
                                solana_client::client_error::ClientErrorKind::Reqwest(e) => {
                                                                results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                                                    "TX - Reqwest Error: {e:?}"
                                                                )))
                                                            }
                                solana_client::client_error::ClientErrorKind::RpcError(e) => match e
                                                            {
                                                                solana_client::rpc_request::RpcError::RpcRequestError(e) => {
                                                                    results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                                                        "TX - RPC Error (Request): {e:?}"
                                                                    )))
                                                                }
                                                                solana_client::rpc_request::RpcError::RpcResponseError {
                                                                    code: _,
                                                                    message: _,
                                                                    data,
                                                                } => {
                                                                    match data {
                                                                        solana_client::rpc_request::RpcResponseErrorData::Empty => {
                                                                            results[idx] = Err(JitoSendTransactionError::TransactionError("TX - RPC Error (Request - Empty)".to_string()))
                                                                        },
                                                                        solana_client::rpc_request::RpcResponseErrorData::SendTransactionPreflightFailure(e) => {
                                                                            debug!("Transaction preflight failure: {e:?}");

                                                                            results[idx] = Err(JitoSendTransactionError::RpcSimulateTransactionResult(e))
                                                                        },
                                                                        solana_client::rpc_request::RpcResponseErrorData::NodeUnhealthy { num_slots_behind } => {
                                                                            results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                                                                "TX - RPC Error (Request - Unhealthy):  slots behind: {num_slots_behind:?}"
                                                                            )))
                                                                        },
                                                                    }
                                                                }
                                                                solana_client::rpc_request::RpcError::ParseError(e) => {
                                                                    results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                                                        "TX - RPC Error (Parse): {e:?}"
                                                                    )))
                                                                }
                                                                solana_client::rpc_request::RpcError::ForUser(e) => {
                                                                    results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                                                        "TX - RPC Error (For User): {e:?}"
                                                                    )))
                                                                }
                                                            },
                                solana_client::client_error::ClientErrorKind::SerdeJson(e) => {
                                                                results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                                                    "TX - Serde Json Error: {e:?}"
                                                                )))
                                                            }
                                solana_client::client_error::ClientErrorKind::SigningError(e) => {
                                                                results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                                                    "TX - Signing Error: {e:?}"
                                                                )))
                                                            }
                                solana_client::client_error::ClientErrorKind::TransactionError(
                                                                e,
                                                            ) => {
                                                                results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                                                    "TX - Transaction Error: {e:?}"
                                                                )))
                                                            }
                                solana_client::client_error::ClientErrorKind::Custom(e) => {
                                                                results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                                                    "TX - Custom Error: {e:?}"
                                                                )))
                                                            }
                                solana_client::client_error::ClientErrorKind::Middleware(e) => {
                                    results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                        "TX - Middleware Error: {e:?}"
                                    )))
                                },
                            }
                        }
                        None => {
                            warn!("Unhandled transaction error: {e:?}");
                            results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                "None transaction error {e:?}"
                            )))
                        }
                    }
                }
            }
        }

        // If all TXs fail preflight, return
        if results.iter().all(|r| {
            matches!(
                r,
                Err(JitoSendTransactionError::RpcSimulateTransactionResult(_))
            )
        }) {
            break;
        }

        tokio::time::sleep(Duration::from_secs(confirmation_time)).await;

        let signatures_to_check: HashSet<Signature> =
            submitted_signatures.clone().into_keys().collect();

        if signatures_to_check.is_empty() {
            break;
        }

        let signatures = parallel_confirm_transactions(client, signatures_to_check).await;

        for signature in signatures {
            results[submitted_signatures[&signature]] = Ok(());
            debug!("Transaction confirmed signature={signature}");
        }

        if results.iter().all(|r| r.is_ok()) {
            break;
        }

        if is_blockhash_not_found
            || !client
                .is_blockhash_valid(&blockhash, CommitmentConfig::processed())
                .await
                .map_err(|e| {
                    JitoTransactionExecutionError::TransactionClientError(
                        e.to_string(),
                        results.clone(),
                    )
                })?
        {
            // Re-sign transactions with fresh blockhash
            let blockhash = get_latest_blockhash_with_retry(client).await.map_err(|e| {
                JitoTransactionExecutionError::TransactionClientError(
                    e.to_string(),
                    results.clone(),
                )
            })?;
            signed_txs = sign_txs(transactions, signer, blockhash);
            retries += 1;
        }
    }

    Ok(results)
}

pub async fn pack_instructions(
    client: &Arc<RpcClient>,
    instructions: &[Instruction],
    signer: &Arc<Keypair>,
    priority_fee_in_microlamports: u64,
    max_cu_per_tx: u32,
) -> Result<Vec<Vec<Instruction>>, Box<dyn std::error::Error>> {
    let mut instructions_with_grouping: Vec<(&Instruction, usize)> = Vec::new();

    for instruction in instructions.iter() {
        let result = find_ix_per_tx(
            client,
            instruction,
            signer,
            priority_fee_in_microlamports,
            max_cu_per_tx,
        )
        .await;

        match result {
            Ok(ix_per_tx) => {
                instructions_with_grouping.push((instruction, ix_per_tx));
            }
            Err(e) => {
                error!("Failed to simulate instruction: {e:?}");
                // Skip this instruction if there is an error
                continue;
            }
        }
    }

    // Group instructions by their grouping size
    let mut grouped_instructions: HashMap<usize, Vec<&Instruction>> = HashMap::new();
    for (instruction, group_size) in instructions_with_grouping {
        grouped_instructions
            .entry(group_size)
            .or_default()
            .push(instruction);
    }

    // Convert HashMap to Vec<Vec<&Instruction>>, ensuring each group meets the length requirement
    let mut result: Vec<Vec<Instruction>> = Vec::new();
    for (group_number, group) in grouped_instructions {
        for chunk in group.chunks(group_number) {
            let mut tx_instructions = Vec::new();
            for instruction in chunk {
                tx_instructions.push((*instruction).clone());
            }
            result.push(tx_instructions);
        }
    }

    Ok(result)
}

#[allow(clippy::too_many_arguments)]
pub async fn parallel_execute_chunk_instructions(
    client: &Arc<RpcClient>,
    instructions: &[Instruction],
    signer: &Arc<Keypair>,
    retry_count: u16,
    confirmation_time: u64,
    priority_fee_in_microlamports: u64,
    max_cu_per_tx: Option<u32>,
    chunk_size: usize,
) -> Result<Vec<Result<(), JitoSendTransactionError>>, JitoTransactionExecutionError> {
    if instructions.is_empty() {
        return Ok(vec![]);
    }

    let max_cu_per_tx = max_cu_per_tx.unwrap_or(DEFAULT_COMPUTE_LIMIT as u32);

    let mut transactions: Vec<Vec<Instruction>> = vec![];

    for ix in instructions.chunks(chunk_size) {
        let mut tx = vec![];
        tx.push(ComputeBudgetInstruction::set_compute_unit_limit(
            DEFAULT_COMPUTE_LIMIT as u32,
        ));
        tx.extend(ix.to_vec());
        transactions.push(tx);
    }

    for tx in transactions.iter_mut() {
        tx.insert(
            0,
            ComputeBudgetInstruction::set_compute_unit_price(priority_fee_in_microlamports),
        );
        if max_cu_per_tx != DEFAULT_COMPUTE_LIMIT as u32 {
            tx.insert(
                0,
                ComputeBudgetInstruction::set_compute_unit_limit(max_cu_per_tx),
            );
        }
    }
    let transactions: Vec<&[Instruction]> = transactions.iter().map(|c| c.as_slice()).collect();

    parallel_execute_transactions(
        client,
        &transactions,
        signer,
        retry_count,
        confirmation_time,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn parallel_execute_instructions(
    client: &Arc<RpcClient>,
    instructions: &[Instruction],
    signer: &Arc<Keypair>,
    retry_count: u16,
    confirmation_time: u64,
    priority_fee_in_microlamports: u64,
    max_cu_per_tx: Option<u32>,
    no_pack: bool,
) -> Result<Vec<Result<(), JitoSendTransactionError>>, JitoTransactionExecutionError> {
    if instructions.is_empty() {
        return Ok(vec![]);
    }

    let max_cu_per_tx = max_cu_per_tx.unwrap_or(DEFAULT_COMPUTE_LIMIT as u32);

    let mut transactions: Vec<Vec<Instruction>> = vec![];

    if no_pack {
        //TODO add option here to chunk X IXs
        for ix in instructions.chunks(1) {
            let mut tx = vec![];
            tx.push(ComputeBudgetInstruction::set_compute_unit_limit(
                DEFAULT_COMPUTE_LIMIT as u32,
            ));
            tx.extend(ix.to_vec());
            transactions.push(tx);
        }
    } else {
        transactions = pack_instructions(
            client,
            instructions,
            signer,
            priority_fee_in_microlamports,
            max_cu_per_tx,
        )
        .await
        .map_err(|e| JitoTransactionExecutionError::ClientError(e.to_string()))?;
    }

    for tx in transactions.iter_mut() {
        tx.insert(
            0,
            ComputeBudgetInstruction::set_compute_unit_price(priority_fee_in_microlamports),
        );
        if max_cu_per_tx != DEFAULT_COMPUTE_LIMIT as u32 {
            tx.insert(
                0,
                ComputeBudgetInstruction::set_compute_unit_limit(max_cu_per_tx),
            );
        }
    }
    let transactions: Vec<&[Instruction]> = transactions.iter().map(|c| c.as_slice()).collect();

    parallel_execute_transactions(
        client,
        &transactions,
        signer,
        retry_count,
        confirmation_time,
    )
    .await
}

pub async fn submit_transactions(
    client: &Arc<RpcClient>,
    transactions: Vec<Vec<Instruction>>,
    keypair: &Arc<Keypair>,
    retry_count: u16,
    confirmation_time: u64,
) -> Result<SubmitStats, JitoTransactionExecutionError> {
    let mut stats = SubmitStats::default();
    let tx_slice = transactions
        .iter()
        .map(|t| t.as_slice())
        .collect::<Vec<_>>();

    match parallel_execute_transactions(client, &tx_slice, keypair, retry_count, confirmation_time)
        .await
    {
        Ok(results) => {
            stats.successes = results.iter().filter(|&tx| tx.is_ok()).count() as u64;
            stats.errors = results.len() as u64 - stats.successes;
            stats.results = results;
            Ok(stats)
        }
        Err(e) => Err(e),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn submit_instructions(
    client: &Arc<RpcClient>,
    instructions: Vec<Instruction>,
    keypair: &Arc<Keypair>,
    priority_fee_in_microlamports: u64,
    retry_count: u16,
    confirmation_time: u64,
    max_cu_per_tx: Option<u32>,
    no_pack: bool,
) -> Result<SubmitStats, JitoTransactionExecutionError> {
    let mut stats = SubmitStats::default();
    match parallel_execute_instructions(
        client,
        &instructions,
        keypair,
        retry_count,
        confirmation_time,
        priority_fee_in_microlamports,
        max_cu_per_tx,
        no_pack,
    )
    .await
    {
        Ok(results) => {
            stats.successes = results.iter().filter(|&tx| tx.is_ok()).count() as u64;
            stats.errors = results.len() as u64 - stats.successes;
            stats.results = results;
            Ok(stats)
        }
        Err(e) => Err(e),
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn submit_chunk_instructions(
    client: &Arc<RpcClient>,
    instructions: Vec<Instruction>,
    keypair: &Arc<Keypair>,
    priority_fee_in_microlamports: u64,
    retry_count: u16,
    confirmation_time: u64,
    max_cu_per_tx: Option<u32>,
    chunk_size: usize,
) -> Result<SubmitStats, JitoTransactionExecutionError> {
    let mut stats = SubmitStats::default();
    match parallel_execute_chunk_instructions(
        client,
        &instructions,
        keypair,
        retry_count,
        confirmation_time,
        priority_fee_in_microlamports,
        max_cu_per_tx,
        chunk_size,
    )
    .await
    {
        Ok(results) => {
            stats.successes = results.iter().filter(|&tx| tx.is_ok()).count() as u64;
            stats.errors = results.len() as u64 - stats.successes;
            stats.results = results;
            Ok(stats)
        }
        Err(e) => Err(e),
    }
}

pub fn configure_instruction(
    ixs: &[Instruction],
    priority_fee: Option<u64>,
    compute_limit: Option<u32>,
    heap_size: Option<u32>,
) -> Vec<Instruction> {
    let mut instructions = ixs.to_vec();
    if let Some(compute_limit) = compute_limit {
        instructions.insert(
            0,
            ComputeBudgetInstruction::set_compute_unit_limit(compute_limit),
        );
    }
    if let Some(priority_fee) = priority_fee {
        instructions.insert(
            0,
            ComputeBudgetInstruction::set_compute_unit_price(priority_fee),
        );
    }
    if let Some(heap_size) = heap_size {
        instructions.insert(0, ComputeBudgetInstruction::request_heap_frame(heap_size));
    }

    instructions
}

pub fn package_instructions(
    ixs: &[Instruction],
    chunk_size: usize,
    priority_fee: Option<u64>,
    compute_limit: Option<u32>,
    heap_size: Option<u32>,
) -> Vec<Vec<Instruction>> {
    ixs.chunks(chunk_size)
        .map(|chunk: &[Instruction]| {
            configure_instruction(chunk, priority_fee, compute_limit, heap_size)
        })
        .collect::<Vec<Vec<Instruction>>>()
}

pub async fn submit_packaged_transactions(
    client: &Arc<RpcClient>,
    transactions: Vec<Vec<Instruction>>,
    keypair: &Arc<Keypair>,
    retry_count: Option<u16>,
    retry_interval: Option<u64>,
) -> Result<SubmitStats, JitoTransactionExecutionError> {
    let mut stats = SubmitStats::default();
    let retry_count = retry_count.unwrap_or(3);
    let retry_interval = retry_interval.unwrap_or(20);

    let mut results = {
        let tx_slice = transactions
            .iter()
            .map(|t| t.as_slice())
            .collect::<Vec<_>>();

        parallel_execute_transactions(client, &tx_slice, keypair, retry_count, retry_interval)
            .await?
    };

    // Instructions carry tuned compute limits, so a transaction whose consumption exceeds
    // the sampled maximum can be rejected for running out of budget. Retry those once at
    // the ceiling rather than dropping the work.
    let exhausted: Vec<usize> = results
        .iter()
        .enumerate()
        .filter(|(_, result)| {
            result
                .as_ref()
                .err()
                .is_some_and(is_compute_budget_exceeded)
        })
        .map(|(index, _)| index)
        .collect();

    if !exhausted.is_empty() {
        warn!(
            "Retrying compute-exhausted transactions count={} limit={}",
            exhausted.len(),
            MAX_COMPUTE_LIMIT
        );

        let raised: Vec<Vec<Instruction>> = exhausted
            .iter()
            .map(|&index| raise_compute_limit(&transactions[index], MAX_COMPUTE_LIMIT))
            .collect();
        let raised_slice = raised.iter().map(|t| t.as_slice()).collect::<Vec<_>>();

        let raised_results = parallel_execute_transactions(
            client,
            &raised_slice,
            keypair,
            retry_count,
            retry_interval,
        )
        .await?;

        for (index, result) in exhausted.into_iter().zip(raised_results) {
            results[index] = result;
        }
    }

    stats.successes = results.iter().filter(|&tx| tx.is_ok()).count() as u64;
    stats.errors = results.len() as u64 - stats.successes;
    stats.results = results;
    Ok(stats)
}

pub fn format_steward_error_log(error: &JitoSendTransactionError) -> String {
    let mut error_logs = String::new();

    match error {
        JitoSendTransactionError::ExceededRetries => {
            error_logs.push_str("Exceeded Retries");
        }
        JitoSendTransactionError::TransactionError(e) => {
            error_logs.push_str(format!("Transaction: {e:?}").as_str());
        }
        JitoSendTransactionError::RpcSimulateTransactionResult(e) => {
            error_logs.push_str("Preflight Error:");

            e.logs.iter().for_each(|log| {
                log.iter().enumerate().for_each(|(i, log)| {
                    error_logs.push_str(format!("{i}: {log:?}").as_str());
                });
            });
        }
    }

    error_logs
}

pub fn print_errors_if_any(submit_stats: &SubmitStats) {
    submit_stats.results.iter().for_each(|result| {
        if let Err(error) = result {
            println!("{}", format_steward_error_log(error));
        }
    });
}

pub fn print_base58_tx(ixs: &[Instruction]) {
    ixs.iter().for_each(|ix| {
        println!("\n------ IX ------\n");

        println!("{}\n", ix.program_id);

        ix.accounts.iter().for_each(|account| {
            let pubkey = format!("{}", account.pubkey);
            let writable = if account.is_writable { "W" } else { "" };
            let signer = if account.is_signer { "S" } else { "" };

            println!("{pubkey:<44} {writable:>2} {signer:>1}");
        });

        println!("\n");

        let base58_string = bs58::encode(&ix.data).into_string();
        println!("{base58_string}\n");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_client::rpc_response::RpcSimulateTransactionResult;

    fn dummy_ix() -> Instruction {
        Instruction {
            program_id: Pubkey::new_unique(),
            accounts: vec![],
            data: vec![7],
        }
    }

    fn sim_result(err: Option<TransactionError>) -> JitoSendTransactionError {
        JitoSendTransactionError::RpcSimulateTransactionResult(RpcSimulateTransactionResult {
            err,
            logs: None,
            accounts: None,
            units_consumed: None,
            loaded_accounts_data_size: None,
            return_data: None,
            inner_instructions: None,
            replacement_blockhash: None,
        })
    }

    #[test]
    fn raise_compute_limit_replaces_existing_limit() {
        let ixs = vec![
            ComputeBudgetInstruction::set_compute_unit_price(200_000),
            ComputeBudgetInstruction::set_compute_unit_limit(39_000),
            dummy_ix(),
        ];

        let raised = raise_compute_limit(&ixs, MAX_COMPUTE_LIMIT);

        // Same instruction count and order, only the limit value changed.
        assert_eq!(raised.len(), 3);
        assert_eq!(raised[0], ixs[0]);
        assert_eq!(
            raised[1],
            ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_LIMIT)
        );
        assert_eq!(raised[2], ixs[2]);
    }

    #[test]
    fn raise_compute_limit_prepends_when_absent() {
        let ixs = vec![
            ComputeBudgetInstruction::set_compute_unit_price(200_000),
            dummy_ix(),
        ];

        let raised = raise_compute_limit(&ixs, MAX_COMPUTE_LIMIT);

        assert_eq!(raised.len(), 3);
        assert_eq!(
            raised[0],
            ComputeBudgetInstruction::set_compute_unit_limit(MAX_COMPUTE_LIMIT)
        );
        assert_eq!(raised[1], ixs[0]);
        assert_eq!(raised[2], ixs[1]);
    }

    #[test]
    fn raise_compute_limit_leaves_price_instruction_alone() {
        // SetComputeUnitPrice is discriminant 3 and must not be mistaken for a limit.
        let ixs = vec![ComputeBudgetInstruction::set_compute_unit_price(200_000)];

        let raised = raise_compute_limit(&ixs, MAX_COMPUTE_LIMIT);

        assert_eq!(raised.len(), 2);
        assert_eq!(raised[1], ixs[0]);
    }

    #[test]
    fn detects_compute_budget_exhaustion_from_preflight() {
        let err = sim_result(Some(TransactionError::InstructionError(
            2,
            InstructionError::ComputationalBudgetExceeded,
        )));
        assert!(is_compute_budget_exceeded(&err));
    }

    #[test]
    fn ignores_unrelated_failures() {
        assert!(!is_compute_budget_exceeded(&sim_result(Some(
            TransactionError::BlockhashNotFound
        ))));
        assert!(!is_compute_budget_exceeded(&sim_result(Some(
            TransactionError::InstructionError(0, InstructionError::Custom(6001))
        ))));
        assert!(!is_compute_budget_exceeded(&sim_result(None)));
        assert!(!is_compute_budget_exceeded(
            &JitoSendTransactionError::ExceededRetries
        ));
        assert!(!is_compute_budget_exceeded(
            &JitoSendTransactionError::TransactionError("TX Error: AccountInUse".to_string())
        ));
    }

    #[test]
    fn detects_compute_budget_exhaustion_from_landed_tx_string() {
        let err = JitoSendTransactionError::TransactionError(
            "TX Error: InstructionError(2, ComputationalBudgetExceeded)".to_string(),
        );
        assert!(is_compute_budget_exceeded(&err));
    }
}
