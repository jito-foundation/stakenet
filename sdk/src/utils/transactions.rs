use std::collections::HashSet;
use std::mem::size_of;
use std::vec;
use std::{collections::HashMap, sync::Arc, time::Duration};

use log::*;
use solana_client::rpc_response::{Response, RpcSimulateTransactionResult, RpcVoteAccountInfo};
use solana_client::{client_error::ClientError, nonblocking::rpc_client::RpcClient};
use solana_metrics::datapoint_error;
use solana_program::hash::Hash;
use solana_sdk::bs58;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::packet::PACKET_DATA_SIZE;
use solana_sdk::transaction::TransactionError;
use solana_sdk::{
    account::Account, commitment_config::CommitmentConfig, instruction::AccountMeta,
    instruction::Instruction, packet::Packet, pubkey::Pubkey, signature::Keypair,
    signature::Signature, signer::Signer, transaction::Transaction,
};
use tokio::task;
use tokio::time::sleep;

use crate::models::errors::{
    JitoMultipleAccountsError, JitoSendTransactionError, JitoTransactionExecutionError,
};
use crate::models::submit_stats::SubmitStats;

use std::future::Future;

pub const DEFAULT_COMPUTE_LIMIT: u64 = 200_000;

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

            if idx % 50 == 0 {
                // Need to avoid spamming the rpc or lots of transactions will get dropped
                sleep(Duration::from_secs(3)).await;
            }

            // Future optimization: submit these in parallel batches and refresh blockhash for every batch
            match client.send_transaction(tx).await {
                Ok(signature) => {
                    debug!("🟨 Submitted: {:?}", signature);
                    println!("🟨 Submitted: {:?}", signature);
                    submitted_signatures.insert(signature, idx);
                }
                Err(e) => {
                    debug!("Transaction error: {:?}", e);
                    match e.get_transaction_error() {
                        Some(TransactionError::BlockhashNotFound) => {
                            debug!("🟧 Blockhash not found");
                            println!("🟧 Blockhash not found");
                            is_blockhash_not_found = true;
                        }
                        Some(TransactionError::AlreadyProcessed) => {
                            debug!("🟪 Already Processed");
                            println!("🟪 Already Processed");
                            submitted_signatures.insert(tx.signatures[0], idx);
                        }
                        Some(_) => {
                            match e.kind {
                                solana_client::client_error::ClientErrorKind::Io(e) => {
                                    results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                        "TX - Io Error: {:?}",
                                        e
                                    )))
                                }
                                solana_client::client_error::ClientErrorKind::Reqwest(e) => {
                                    results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                        "TX - Reqwest Error: {:?}",
                                        e
                                    )))
                                }
                                solana_client::client_error::ClientErrorKind::RpcError(e) => match e
                                {
                                    solana_client::rpc_request::RpcError::RpcRequestError(e) => {
                                        results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                            "TX - RPC Error (Request): {:?}",
                                            e
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
                                                println!("🟥 Preflight Error: \n{:?}\n\n", e);

                                                results[idx] = Err(JitoSendTransactionError::RpcSimulateTransactionResult(e))
                                            },
                                            solana_client::rpc_request::RpcResponseErrorData::NodeUnhealthy { num_slots_behind } => {
                                                results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                                    "TX - RPC Error (Request - Unhealthy):  slots behind: {:?}",
                                                    num_slots_behind
                                                )))
                                            },
                                        }
                                    }
                                    solana_client::rpc_request::RpcError::ParseError(e) => {
                                        results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                            "TX - RPC Error (Parse): {:?}",
                                            e
                                        )))
                                    }
                                    solana_client::rpc_request::RpcError::ForUser(e) => {
                                        results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                            "TX - RPC Error (For User): {:?}",
                                            e
                                        )))
                                    }
                                },
                                solana_client::client_error::ClientErrorKind::SerdeJson(e) => {
                                    results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                        "TX - Serde Json Error: {:?}",
                                        e
                                    )))
                                }
                                solana_client::client_error::ClientErrorKind::SigningError(e) => {
                                    results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                        "TX - Signing Error: {:?}",
                                        e
                                    )))
                                }
                                solana_client::client_error::ClientErrorKind::TransactionError(
                                    e,
                                ) => {
                                    results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                        "TX - Transaction Error: {:?}",
                                        e
                                    )))
                                }
                                solana_client::client_error::ClientErrorKind::Custom(e) => {
                                    results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                        "TX - Custom Error: {:?}",
                                        e
                                    )))
                                }
                            }
                        }
                        None => {
                            warn!("None Transaction error: {:?}", e);
                            results[idx] = Err(JitoSendTransactionError::TransactionError(format!(
                                "None transaction error {:?}",
                                e
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
            debug!("🟩 Completed: {:?}", signature);
            println!("🟩 Completed: {:?}", signature);
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
    no_pack: bool,
) -> Result<Vec<Result<(), JitoSendTransactionError>>, JitoTransactionExecutionError> {
    if instructions.is_empty() {
        return Ok(vec![]);
    }

    let max_cu_per_tx = max_cu_per_tx.unwrap_or(DEFAULT_COMPUTE_LIMIT as u32);

    let mut transactions: Vec<Vec<Instruction>> = vec![];

    if no_pack {
        for ix in instructions.iter() {
            transactions.push(vec![ix.clone()]);
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
) -> Result<SubmitStats, JitoTransactionExecutionError> {
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
    no_pack: bool,
) -> Result<SubmitStats, JitoTransactionExecutionError> {
    let mut stats = SubmitStats::default();
    match parallel_execute_instructions(
        client,
        &instructions,
        keypair,
        100,
        20,
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
    let tx_slice = transactions
        .iter()
        .map(|t| t.as_slice())
        .collect::<Vec<_>>();

    match parallel_execute_transactions(
        client,
        &tx_slice,
        keypair,
        retry_count.unwrap_or(3),
        retry_interval.unwrap_or(20),
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

pub fn format_steward_error_log(error: &JitoSendTransactionError) -> String {
    let mut error_logs = String::new();

    match error {
        JitoSendTransactionError::ExceededRetries => {
            error_logs.push_str("Exceeded Retries");
        }
        JitoSendTransactionError::TransactionError(e) => {
            error_logs.push_str(format!("Transaction: {:?}", e).as_str());
        }
        JitoSendTransactionError::RpcSimulateTransactionResult(e) => {
            error_logs.push_str("Preflight Error:");

            e.logs.iter().for_each(|log| {
                log.iter().enumerate().for_each(|(i, log)| {
                    error_logs.push_str(format!("{}: {:?}", i, log).as_str());
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

            println!("{:<44} {:>2} {:>1}", pubkey, writable, signer);
        });

        println!("\n");

        let base58_string = bs58::encode(&ix.data).into_string();
        println!("{}\n", base58_string);
    });
}
