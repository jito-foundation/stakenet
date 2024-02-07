use std::fmt::{Display, Formatter};
use std::mem::size_of;
use std::vec;
use std::{collections::HashMap, sync::Arc, time::Duration};

use clap::ValueEnum;
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

#[derive(Debug, Default, Clone)]
pub struct SubmitStats {
    pub successes: u64,
    pub errors: u64,
    pub results: Vec<Result<(), SendTransactionError>>,
}
#[derive(Debug, Default, Clone)]
pub struct CreateUpdateStats {
    pub creates: SubmitStats,
    pub updates: SubmitStats,
}

pub type Error = Box<dyn std::error::Error>;
#[derive(ThisError, Debug, Clone)]
pub enum TransactionExecutionError {
    #[error("RPC Client error: {0:?}")]
    ClientError(String),
    #[error("RPC Client error: {0:?}")]
    TransactionClientError(String, Vec<Result<(), SendTransactionError>>),
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

async fn parallel_confirm_transactions(
    client: &RpcClient,
    executed_signatures: HashMap<Signature, usize>,
) -> HashMap<Signature, usize> {
    // Confirmes TXs in batches of 256 (max allowed by RPC method). Returns confirmed signatures
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
    let mut confirmed_signatures: HashMap<Signature, usize> = HashMap::new();
    for result_batch in results.iter() {
        for (sig, result) in result_batch {
            if let Some(status) = result {
                if status.satisfies_commitment(client.commitment()) && status.err.is_none() {
                    confirmed_signatures.insert(*sig, executed_signatures[sig]);
                }
            }
        }
    }

    info!(
        "{} transactions submitted, {} confirmed",
        num_transactions_submitted,
        confirmed_signatures.len()
    );
    confirmed_signatures.clone()
}

async fn sign_txs(
    client: &Arc<RpcClient>,
    transactions: &[&[Instruction]],
    signer: &Arc<Keypair>,
) -> Result<Vec<Transaction>, ClientError> {
    let blockhash = get_latest_blockhash_with_retry(client).await?;

    let signed_txs = transactions
        .into_iter()
        .map(|instructions| {
            Transaction::new_signed_with_payer(
                instructions,
                Some(&signer.pubkey()),
                &[signer.as_ref()],
                blockhash,
            )
        })
        .collect();

    Ok(signed_txs)
}

#[derive(Clone, Debug)]
pub enum SendTransactionError {
    ExceededRetries,
    // Stores ClientError.to_string(), since ClientError does not impl Clone, and we want to track both
    // io/reqwest errors as well as transaction errors
    TransactionError(String),
}

pub async fn parallel_execute_transactions(
    client: &Arc<RpcClient>,
    transactions: &[&[Instruction]],
    signer: &Arc<Keypair>,
    retry_count: u16,
    confirmation_time: u64,
) -> Result<Vec<Result<(), SendTransactionError>>, TransactionExecutionError> {
    let mut results = vec![Err(SendTransactionError::ExceededRetries); transactions.len()];
    let mut retries = 0;

    if transactions.is_empty() {
        return Ok(results);
    }

    let mut signed_txs = sign_txs(client, transactions, signer)
        .await
        .map_err(|e| TransactionExecutionError::ClientError(e.to_string()))?;

    while retries < retry_count {
        let mut submitted_signatures = HashMap::new();

        for (idx, tx) in signed_txs.iter().enumerate() {
            if results[idx].is_ok() {
                continue;
            }

            // Future optimization: submit these in parallel batches and refresh blockhash for every batch
            match client.send_transaction(tx).await {
                Ok(signature) => {
                    submitted_signatures.insert(signature, idx);
                }
                Err(e) => match e.get_transaction_error() {
                    Some(TransactionError::BlockhashNotFound)
                    | Some(TransactionError::AlreadyProcessed) => {
                        submitted_signatures.insert(tx.signatures[0], idx);
                    }
                    Some(_) | None => {
                        warn!("Transaction error: {}", e.to_string());
                        results[idx] = Err(SendTransactionError::TransactionError(e.to_string()))
                    }
                },
            }
        }

        tokio::time::sleep(Duration::from_secs(confirmation_time)).await;

        for idx in parallel_confirm_transactions(client, submitted_signatures)
            .await
            .into_values()
        {
            results[idx] = Ok(());
        }

        if results.iter().all(|r| r.is_ok()) {
            break;
        }

        // Re-sign transactions with fresh blockhash
        signed_txs = sign_txs(client, transactions, signer).await.map_err(|e| {
            TransactionExecutionError::TransactionClientError(e.to_string(), results.clone())
        })?;
        retries += 1;
    }

    Ok(results)
}

pub async fn parallel_execute_instructions(
    client: &Arc<RpcClient>,
    instructions: &[Instruction],
    signer: &Arc<Keypair>,
    retry_count: u16,
    confirmation_time: u64,
) -> Result<Vec<Result<(), SendTransactionError>>, TransactionExecutionError> {
    /*
        Note: Assumes all instructions are equivalent in compute, equivalent in size, and can be executed in any order

        1) Submits all instructions in parallel
        2) Waits a bit for them to confirm
        3) Checks which ones have confirmed, and keeps the ones that haven't
        4) Repeats retry_count number of times until all have confirmed

        Returns all remaining instructions that haven't executed so application can handle
    */

    if instructions.is_empty() {
        return Ok(vec![]);
    }

    let instructions_per_tx = calculate_instructions_per_tx(client, &instructions, signer)
        .await
        .map_err(|e| TransactionExecutionError::ClientError(e.to_string()))?;
    let transactions: Vec<&[Instruction]> = instructions.chunks(instructions_per_tx).collect();

    parallel_execute_transactions(
        client,
        &transactions,
        signer,
        retry_count,
        confirmation_time,
    )
    .await
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
) -> Result<SubmitStats, TransactionExecutionError> {
    let mut stats = SubmitStats::default();
    let num_transactions = transactions.len();
    let tx_slice = transactions
        .iter()
        .map(|t| t.as_slice())
        .collect::<Vec<_>>();

    match parallel_execute_transactions(client, &tx_slice, keypair, 10, 30).await {
        Ok(results) => {
            stats.successes = results.iter().filter(|&tx| tx.is_ok()).count() as u64;
            stats.errors = num_transactions as u64 - stats.successes;
            stats.results = results;
            Ok(stats)
        }
        Err(e) => Err(e),
    }
}

pub async fn submit_instructions(
    client: &Arc<RpcClient>,
    instructions: Vec<Instruction>,
    keypair: &Arc<Keypair>,
) -> Result<SubmitStats, TransactionExecutionError> {
    let mut stats = SubmitStats::default();
    let num_instructions = instructions.len();
    match parallel_execute_instructions(client, &instructions, keypair, 10, 30).await {
        Ok(results) => {
            stats.successes = results.iter().filter(|&tx| tx.is_ok()).count() as u64;
            stats.errors = num_instructions as u64 - stats.successes;
            stats.results = results;
            Ok(stats)
        }
        Err(e) => Err(e),
    }
}

pub async fn submit_create_and_update(
    client: &Arc<RpcClient>,
    create_transactions: Vec<Vec<Instruction>>,
    update_instructions: Vec<Instruction>,
    keypair: &Arc<Keypair>,
) -> Result<CreateUpdateStats, TransactionExecutionError> {
    let mut stats = CreateUpdateStats::default();
    stats.creates = submit_transactions(client, create_transactions, keypair).await?;
    stats.updates = submit_instructions(client, update_instructions, keypair).await?;
    Ok(stats)
}
