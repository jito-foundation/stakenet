use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::mem::size_of;
use std::vec;
use std::{collections::HashMap, sync::Arc, time::Duration};

use clap::ValueEnum;
use log::*;
use solana_client::rpc_response::{Response, RpcSimulateTransactionResult, RpcVoteAccountInfo};
use solana_client::{client_error::ClientError, nonblocking::rpc_client::RpcClient};
use solana_metrics::datapoint_error;
use solana_program::hash::Hash;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::transaction::TransactionError;
use solana_sdk::{
    account::Account, commitment_config::CommitmentConfig, instruction::AccountMeta,
    instruction::Instruction, packet::Packet, pubkey::Pubkey, signature::Keypair,
    signature::Signature, signer::Signer, transaction::Transaction,
};
use thiserror::Error as ThisError;
use tokio::task::{self, JoinError};
use tokio::time::sleep;

const DEFAULT_COMPUTE_LIMIT: usize = 200_000;

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

#[derive(ThisError, Clone, Debug)]
pub enum SendTransactionError {
    #[error("Exceeded retries")]
    ExceededRetries,
    // Stores ClientError.to_string(), since ClientError does not impl Clone, and we want to track both
    // io/reqwest errors as well as transaction errors
    #[error("Transaction error: {0}")]
    TransactionError(String),
}

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

async fn simulate_instruction(
    client: &RpcClient,
    instruction: &Instruction,
    signer: &Keypair,
    priority_fee_in_microlamports: u64,
    max_cu_per_tx: u32,
) -> Result<Response<RpcSimulateTransactionResult>, ClientError> {
    let latest_blockhash = get_latest_blockhash_with_retry(client).await?;

    let test_tx = Transaction::new_signed_with_payer(
        &[
            ComputeBudgetInstruction::set_compute_unit_limit(max_cu_per_tx),
            ComputeBudgetInstruction::set_compute_unit_price(priority_fee_in_microlamports),
            instruction.to_owned(),
        ],
        Some(&signer.pubkey()),
        &[signer],
        latest_blockhash,
    );

    client.simulate_transaction(&test_tx).await
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
        error!("Simulation error: {} {:?}", max_cu_per_tx, response.value);

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
        .unwrap_or(DEFAULT_COMPUTE_LIMIT as u64);

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

    let mut confirmed_signatures: HashSet<Signature> = HashSet::new();
    for result_batch in results.iter() {
        for (sig, result) in result_batch {
            if let Some(status) = result {
                if status.satisfies_commitment(client.commitment()) && status.err.is_none() {
                    confirmed_signatures.insert(*sig);
                }
            }
        }
    }

    info!(
        "{} transactions submitted, {} confirmed",
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

    let blockhash = get_latest_blockhash_with_retry(client)
        .await
        .map_err(|e| TransactionExecutionError::ClientError(e.to_string()))?;
    let mut signed_txs = sign_txs(transactions, signer, blockhash);

    while retries < retry_count {
        let mut submitted_signatures = HashMap::new();
        let mut is_blockhash_not_found = false;

        for (idx, tx) in signed_txs.iter().enumerate() {
            if results[idx].is_ok() {
                continue;
            }
            if idx % 50 == 0 {
                // Need to avoid spamming the rpc or lots of transactions will get dropped
                sleep(Duration::from_secs(3)).await;
            }

            // Future optimization: submit these in parallel batches and refresh blockhash for every batch
            match client.send_transaction(tx).await {
                Ok(signature) => {
                    submitted_signatures.insert(signature, idx);
                }
                Err(e) => match e.get_transaction_error() {
                    Some(TransactionError::BlockhashNotFound) => {
                        is_blockhash_not_found = true;
                    }
                    Some(TransactionError::AlreadyProcessed) => {
                        submitted_signatures.insert(tx.signatures[0], idx);
                    }
                    Some(_) | None => {
                        warn!("Transaction error: {:?}", e);
                        results[idx] = Err(SendTransactionError::TransactionError(e.to_string()))
                    }
                },
            }
        }

        tokio::time::sleep(Duration::from_secs(confirmation_time)).await;

        for signature in parallel_confirm_transactions(
            client,
            submitted_signatures.clone().into_keys().collect(),
        )
        .await
        {
            results[submitted_signatures[&signature]] = Ok(());
        }

        if results.iter().all(|r| r.is_ok()) {
            break;
        }

        if is_blockhash_not_found
            || !client
                .is_blockhash_valid(&blockhash, CommitmentConfig::processed())
                .await
                .map_err(|e| {
                    TransactionExecutionError::TransactionClientError(
                        e.to_string(),
                        results.clone(),
                    )
                })?
        {
            // Re-sign transactions with fresh blockhash
            let blockhash = get_latest_blockhash_with_retry(client).await.map_err(|e| {
                TransactionExecutionError::TransactionClientError(e.to_string(), results.clone())
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
                error!("Could not simulate instruction: {:?}", e);
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

pub async fn parallel_execute_instructions(
    client: &Arc<RpcClient>,
    instructions: &[Instruction],
    signer: &Arc<Keypair>,
    retry_count: u16,
    confirmation_time: u64,
    priority_fee_in_microlamports: u64,
    max_cu_per_tx: Option<u32>,
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

    // let instructions_per_tx = calculate_instructions_per_tx(
    //     client,
    //     instructions,
    //     signer,
    //     priority_fee_in_microlamports,
    //     max_cu_per_tx,
    // )
    // .await
    // .map_err(|e| TransactionExecutionError::ClientError(e.to_string()))?
    //     - 1;

    let max_cu_per_tx = max_cu_per_tx.unwrap_or(DEFAULT_COMPUTE_LIMIT as u32);

    let mut transactions: Vec<Vec<Instruction>> = pack_instructions(
        client,
        instructions,
        signer,
        priority_fee_in_microlamports,
        max_cu_per_tx,
    )
    .await
    .map_err(|e| TransactionExecutionError::ClientError(e.to_string()))?;

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
    let tx_slice = transactions
        .iter()
        .map(|t| t.as_slice())
        .collect::<Vec<_>>();

    match parallel_execute_transactions(client, &tx_slice, keypair, 100, 20).await {
        Ok(results) => {
            stats.successes = results.iter().filter(|&tx| tx.is_ok()).count() as u64;
            stats.errors = results.len() as u64 - stats.successes;
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
    priority_fee_in_microlamports: u64,
    max_cu_per_tx: Option<u32>,
) -> Result<SubmitStats, TransactionExecutionError> {
    let mut stats = SubmitStats::default();
    match parallel_execute_instructions(
        client,
        &instructions,
        keypair,
        100,
        20,
        priority_fee_in_microlamports,
        max_cu_per_tx,
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

pub async fn submit_create_and_update(
    client: &Arc<RpcClient>,
    create_transactions: Vec<Vec<Instruction>>,
    update_instructions: Vec<Instruction>,
    keypair: &Arc<Keypair>,
    priority_fee_in_microlamports: u64,
    max_cu_per_tx: Option<u32>,
) -> Result<CreateUpdateStats, TransactionExecutionError> {
    Ok(CreateUpdateStats {
        creates: submit_transactions(client, create_transactions, keypair).await?,
        updates: submit_instructions(
            client,
            update_instructions,
            keypair,
            priority_fee_in_microlamports,
            max_cu_per_tx,
        )
        .await?,
    })
}
