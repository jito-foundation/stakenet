use std::borrow::BorrowMut;
use std::fmt::{Display, Formatter};
use std::mem::size_of;
use std::sync::Mutex;
use std::vec;
use std::{collections::HashMap, sync::Arc, time::Duration};

use clap::{error, ValueEnum};
use log::*;
use solana_client::rpc_response::RpcVoteAccountInfo;
use solana_client::{client_error::ClientError, nonblocking::rpc_client::RpcClient};
use solana_program::hash::Hash;
use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::transaction::TransactionError;
use solana_sdk::{
    account::Account, commitment_config::CommitmentConfig, instruction::AccountMeta,
    instruction::Instruction, packet::Packet, pubkey::Pubkey, signature::Keypair,
    signature::Signature, signer::Signer, transaction::Transaction,
};
use thiserror::Error as ThisError;
use tokio::task::{self, JoinError};

#[derive(Debug, Default, Clone, Copy)]
pub struct SubmitStats {
    pub successes: u64,
    pub errors: u64,
}
#[derive(Debug, Default, Clone, Copy)]
pub struct CreateUpdateStats {
    pub creates: SubmitStats,
    pub updates: SubmitStats,
}

pub type Error = Box<dyn std::error::Error>;
#[derive(ThisError, Debug, Clone)]
pub enum TransactionExecutionError {
    #[error("Transactions failed to execute after multiple retries")]
    TransactionRetryError(Vec<(Vec<Instruction>, String)>),
    #[error("RPC Client error: {0:?}")]
    ClientError(String, Vec<Instruction>),
    #[error("RPC Client error: {0:?}")]
    TransactionClientError(String, Vec<Vec<Instruction>>),
}

pub const NOT_CONFIRMED_MESSAGE: &str = "Transaction failed to confirm after multiple retries";

#[derive(ThisError, Debug)]
pub enum MultipleAccountsError {
    #[error(transparent)]
    ClientError(#[from] ClientError),
    #[error(transparent)]
    JoinError(#[from] JoinError),
}

#[derive(ValueEnum, Debug, Clone)]
pub enum Cluster {
    Mainnet,
    Testnet,
    Localnet,
}

impl Display for Cluster {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Cluster::Mainnet => write!(f, "mainnet"),
            Cluster::Testnet => write!(f, "testnet"),
            Cluster::Localnet => write!(f, "localnet"),
        }
    }
}

pub trait CreateTransaction {
    fn create_transaction(&self) -> Vec<Instruction>;
}

pub trait UpdateInstruction {
    fn update_instruction(&self) -> Instruction;
}

pub trait Address {
    fn address(&self) -> Pubkey;
}

pub async fn get_multiple_accounts_batched(
    accounts: &[Pubkey],
    rpc_client: &Arc<RpcClient>,
) -> Result<Vec<Option<Account>>, MultipleAccountsError> {
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
                return Err(MultipleAccountsError::ClientError(e));
            }
            Err(e) => return Err(MultipleAccountsError::JoinError(e)),
        }
    }
    Ok(accounts_result)
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

const DEFAULT_COMPUTE_LIMIT: usize = 200_000;

async fn calculate_instructions_per_tx(
    client: &RpcClient,
    instructions: &[Instruction],
    signer: &Keypair,
) -> Result<usize, ClientError> {
    let blockhash = get_latest_blockhash_with_retry(client).await?;
    let test_tx = Transaction::new_signed_with_payer(
        &[instructions[0].to_owned()],
        Some(&signer.pubkey()),
        &[signer],
        blockhash,
    );
    let response = client.simulate_transaction(&test_tx).await?;
    if let Some(err) = response.value.clone().err {
        debug!("Simulation error: {:?}", response.value);
        return Err(err.into());
    }
    let compute = response
        .value
        .units_consumed
        .unwrap_or(DEFAULT_COMPUTE_LIMIT as u64);

    let serialized_size = Packet::from_data(None, &test_tx).unwrap().meta().size;
    // additional size per ix
    let size_per_ix =
        instructions[0].accounts.len() * size_of::<AccountMeta>() + instructions[0].data.len();
    let size_max = (PACKET_DATA_SIZE - serialized_size + size_per_ix) / size_per_ix;

    let compute_max = DEFAULT_COMPUTE_LIMIT / compute as usize;

    Ok(size_max.min(compute_max))
}

async fn parallel_submit_transactions(
    client: &RpcClient,
    signer: &Arc<Keypair>,
    // Each &[Instruction] represents a transaction
    transactions: &[&[Instruction]],
) -> Result<(HashMap<Signature, usize>, HashMap<usize, String>), TransactionExecutionError> {
    // Converts arrays of instructions into transactions and submits them in parallel, in batches of 50 (arbitrary, to avoid spamming RPC)
    // Saves signatures associated with the indexes of instructions it contains. Drops transactions that fail simulation, unless the error is BlockhashNotFound
    // Returns a hashmap of executed signatures and their indexes, and a hashmap of errors and their indexes

    let mut executed_signatures: HashMap<Signature, usize> = HashMap::new();
    let mut error_messages: HashMap<usize, String> = HashMap::new();

    const TX_BATCH_SIZE: usize = 50;
    for (batch_num, transaction_batch) in transactions.chunks(TX_BATCH_SIZE).enumerate() {
        let index_offset = TX_BATCH_SIZE * batch_num;
        let recent_blockhash = get_latest_blockhash_with_retry(client).await.map_err(|e| {
            TransactionExecutionError::TransactionClientError(
                e.to_string(),
                transactions.iter().map(|&tx| tx.to_vec()).collect(),
            )
        })?;
        // Convert instructions to transactions in batches and send them all, saving their signatures
        let transactions: Vec<Transaction> = transaction_batch
            .iter()
            .map(|batch| {
                Transaction::new_signed_with_payer(
                    batch,
                    Some(&signer.pubkey()),
                    &[signer.as_ref()],
                    recent_blockhash,
                )
            })
            .collect();

        let tx_futures = transactions
            .iter()
            .map(|tx| async move { client.send_transaction(tx).await })
            .collect::<Vec<_>>();

        let results = futures::future::join_all(tx_futures).await;
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(signature) => {
                    executed_signatures.insert(signature, i + index_offset);
                }
                Err(e) => {
                    match e.get_transaction_error() {
                        // If blockhash not found, transaction is still valid and should not be dropped
                        Some(TransactionError::BlockhashNotFound) => {
                            executed_signatures.insert(
                                transactions[i + index_offset].signatures[0],
                                i + index_offset,
                            );
                        }
                        // If another error is returned, transaction probably won't succeed on retries
                        Some(_) | None => {
                            let message = e.to_string();
                            warn!("Transaction failed: {}", message);
                            error_messages.insert(i + index_offset, message);
                        }
                    }
                }
            }
        }

        debug!(
            "Transactions sent: {}, executed_signatures: {}",
            transactions.len(),
            executed_signatures.len()
        );
    }

    Ok((executed_signatures, error_messages))
}

async fn parallel_confirm_transactions(
    client: &RpcClient,
    mut executed_signatures: HashMap<Signature, usize>,
) -> HashMap<Signature, usize> {
    // Confirmes TXs in batches of 256 (max allowed by RPC method)
    const SIG_STATUS_BATCH_SIZE: usize = 256;
    let signatures_to_confirm = executed_signatures.clone().into_keys().collect::<Vec<_>>();

    // Imperfect logic here: if a transaction is slow to confirm on first submission, and it can only be called once succesfully,
    // it will be resubmitted and fail. Ideally on the next loop it will not be included in the instructions list
    let confirmation_futures: Vec<_> = signatures_to_confirm
        .chunks(SIG_STATUS_BATCH_SIZE)
        .map(|sig_batch| async move {
            match client.get_signature_statuses(sig_batch).await {
                Ok(sig_batch_response) => sig_batch_response
                    .value
                    .iter()
                    .enumerate()
                    .map(|(i, sig_status)| (sig_batch[i], sig_status.clone()))
                    .collect::<Vec<_>>(),
                Err(_) => vec![],
            }
        })
        .collect();

    let results = futures::future::join_all(confirmation_futures).await;

    let num_transactions_submitted = executed_signatures.len();
    for result_batch in results.iter() {
        for (sig, result) in result_batch {
            if let Some(status) = result {
                if status.satisfies_commitment(client.commitment()) && status.err.is_none() {
                    executed_signatures.remove(sig);
                }
            }
        }
    }

    info!(
        "{} transactions submitted, {} confirmed",
        num_transactions_submitted,
        num_transactions_submitted - executed_signatures.len()
    );
    executed_signatures.clone()
}

pub async fn parallel_execute_instructions(
    client: &Arc<RpcClient>,
    instructions: Vec<Instruction>,
    signer: &Arc<Keypair>,
    retry_count: u16,
    confirmation_time: u64,
) -> Result<(), TransactionExecutionError> {
    /*
        Note: Assumes all instructions are equivalent in compute, equivalent in size, and can be executed in any order

        1) Submits all instructions in parallel
        2) Waits a bit for them to confirm
        3) Checks which ones have confirmed, and keeps the ones that haven't
        4) Repeats retry_count number of times until all have confirmed

        Returns all remaining instructions that haven't executed so application can handle
    */

    if instructions.is_empty() {
        return Ok(());
    }

    let instructions_per_tx = calculate_instructions_per_tx(client, &instructions, signer)
        .await
        .map_err(|e| {
            TransactionExecutionError::ClientError(e.to_string(), instructions.to_vec())
        })?;
    let transactions: Vec<&[Instruction]> = instructions.chunks(instructions_per_tx).collect();

    parallel_execute_transactions(client, transactions, signer, retry_count, confirmation_time)
        .await
}

pub async fn parallel_execute_transactions(
    client: &Arc<RpcClient>,
    mut transactions: Vec<&[Instruction]>,
    signer: &Arc<Keypair>,
    retry_count: u16,
    confirmation_time: u64,
) -> Result<(), TransactionExecutionError> {
    // Accepts a list of transactions (each represented as &[Instruction])
    // Executes them in parallel, returns the ones that failed to execute
    // And repeats up to retry_count number of times until all have executed
    if transactions.is_empty() {
        return Ok(());
    }

    let mut error_messages = HashMap::new();

    for _ in 0..retry_count {
        let (executed_signatures, current_error_messages) =
            parallel_submit_transactions(client, signer, &transactions).await?;
        error_messages.extend(current_error_messages);

        tokio::time::sleep(Duration::from_secs(confirmation_time)).await;

        let remaining_signatures =
            parallel_confirm_transactions(client, executed_signatures.clone()).await;

        // All have been executed
        if remaining_signatures.is_empty() {
            return Ok(());
        }

        // Update transactions to the ones remaining
        transactions = executed_signatures
            .into_values()
            .map(|i| transactions[i])
            .collect::<Vec<_>>();
    }

    let not_confirmed_message = NOT_CONFIRMED_MESSAGE.to_string();
    Err(TransactionExecutionError::TransactionRetryError(
        transactions
            .iter()
            .enumerate()
            .map(|(i, tx)| {
                let message = error_messages.get(&i).unwrap_or(&not_confirmed_message);
                (tx.to_vec(), message.clone())
            })
            .collect::<Vec<_>>(),
    ))
}

pub async fn build_create_and_update_instructions<
    T: Address + CreateTransaction + UpdateInstruction,
>(
    client: &Arc<RpcClient>,
    account_entries: &[T],
) -> Result<(Vec<Vec<Instruction>>, Vec<Instruction>), MultipleAccountsError> {
    let addresses = account_entries
        .iter()
        .map(|a| a.address())
        .collect::<Vec<Pubkey>>();
    let existing_accounts_response: Vec<Option<Account>> =
        get_multiple_accounts_batched(&addresses, client).await?;

    let create_transactions = existing_accounts_response
        .iter()
        .zip(account_entries.iter())
        .filter_map(|(existing_account, entry)| {
            if existing_account.is_none() {
                Some(entry.create_transaction())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    Ok((
        create_transactions,
        account_entries
            .iter()
            .map(|entry| entry.update_instruction())
            .collect(),
    ))
}

pub async fn submit_transactions(
    client: &Arc<RpcClient>,
    transactions: Vec<Vec<Instruction>>,
    keypair: &Arc<Keypair>,
) -> Result<SubmitStats, (TransactionExecutionError, SubmitStats)> {
    let mut stats = SubmitStats::default();
    let num_transactions = transactions.len();
    match parallel_execute_transactions(
        client,
        transactions.iter().map(AsRef::as_ref).collect(),
        keypair,
        10,
        60,
    )
    .await
    {
        Ok(_) => {
            stats.successes = num_transactions as u64;
        }
        Err(e) => {
            let transactions_len = match e.clone() {
                TransactionExecutionError::TransactionRetryError(transactions) => {
                    transactions.len()
                }
                TransactionExecutionError::TransactionClientError(_, transactions) => {
                    transactions.len()
                }
                _ => {
                    error!("Hit unreachable statement in submit_transactions");
                    unreachable!();
                }
            };
            stats.successes = num_transactions as u64 - transactions_len as u64;
            stats.errors = transactions_len as u64;
            return Err((e, stats));
        }
    }
    Ok(stats)
}

pub async fn submit_instructions(
    client: &Arc<RpcClient>,
    instructions: Vec<Instruction>,
    keypair: &Arc<Keypair>,
) -> Result<SubmitStats, (TransactionExecutionError, SubmitStats)> {
    let mut stats = SubmitStats::default();
    let num_instructions = instructions.len();
    match parallel_execute_instructions(client, instructions, keypair, 10, 30).await {
        Ok(_) => {
            stats.successes = num_instructions as u64;
        }
        Err(e) => {
            let instructions_len = match e.clone() {
                TransactionExecutionError::ClientError(_, instructions) => instructions.len(),
                TransactionExecutionError::TransactionClientError(_, instructions) => {
                    instructions.concat().len()
                }
                _ => {
                    error!("Hit unreachable statement in submit_instructions");
                    unreachable!();
                }
            };
            stats.successes = num_instructions as u64 - instructions_len as u64;
            stats.errors = instructions_len as u64;
            return Err((e, stats));
        }
    }
    Ok(stats)
}

pub async fn submit_create_and_update(
    client: &Arc<RpcClient>,
    create_transactions: Vec<Vec<Instruction>>,
    update_instructions: Vec<Instruction>,
    keypair: &Arc<Keypair>,
) -> Result<CreateUpdateStats, (TransactionExecutionError, CreateUpdateStats)> {
    let mut stats = CreateUpdateStats::default();
    stats.creates = submit_transactions(client, create_transactions, keypair)
        .await
        .map_err(|(e, submit_stats)| {
            stats.creates = submit_stats;
            (e, stats)
        })?;
    stats.updates = submit_instructions(client, update_instructions, keypair)
        .await
        .map_err(|(e, submit_stats)| {
            stats.updates = submit_stats;
            (e, stats)
        })?;
    Ok(stats)
}
