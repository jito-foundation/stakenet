use std::sync::Arc;

use crate::commands::command_args::TransactionParameters;
use anyhow::Result;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use borsh::BorshSerialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, pubkey::Pubkey, signature::Keypair, signer::Signer,
    transaction::Transaction,
};
use stakenet_sdk::{
    models::{errors::JitoTransactionExecutionError, submit_stats::SubmitStats},
    utils::transactions::parallel_execute_transactions,
};
use std::io::Cursor;

/// Decides whether to print the transaction in raw or governance format.
/// Returns true if a print happened and caller should skip executing.
pub fn maybe_print_tx(ixs: &[Instruction], params: &TransactionParameters) -> bool {
    if params.print_tx {
        stakenet_sdk::utils::transactions::print_base58_tx(ixs);
        true
    } else if params.print_gov_tx {
        print_governance_ix(ixs);
        true
    } else {
        false
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

pub async fn debug_send_single_transaction(
    client: &Arc<RpcClient>,
    payer: &Arc<Keypair>,
    instructions: &[Instruction],
    debug_print: Option<bool>,
) -> Result<solana_sdk::signature::Signature, solana_client::client_error::ClientError> {
    let transaction = Transaction::new_signed_with_payer(
        instructions,
        Some(&payer.pubkey()),
        &[&payer],
        client.get_latest_blockhash().await?,
    );

    let result = client.send_and_confirm_transaction(&transaction).await;

    if debug_print.unwrap_or(false) {
        match &result {
            Ok(signature) => {
                println!("Signature: {signature}");
            }
            Err(e) => {
                println!("Accounts: {:?}", &instructions.last().unwrap().accounts);
                println!("Error: {e:?}");
            }
        }
    }

    result
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize)]
pub struct InstructionData {
    /// Pubkey of the instruction processor that executes this instruction
    pub program_id: Pubkey,
    /// Metadata for what accounts should be passed to the instruction processor
    pub accounts: Vec<AccountMetaData>,
    /// Opaque data passed to the instruction processor
    pub data: Vec<u8>,
}

/// Account metadata used to define Instructions
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize)]
pub struct AccountMetaData {
    /// An account's public key
    pub pubkey: Pubkey,
    /// True if an Instruction requires a Transaction signature matching
    /// `pubkey`.
    pub is_signer: bool,
    /// True if the `pubkey` can be loaded as a read-write account.
    pub is_writable: bool,
}

impl From<Instruction> for InstructionData {
    fn from(instruction: Instruction) -> Self {
        InstructionData {
            program_id: instruction.program_id,
            accounts: instruction
                .accounts
                .iter()
                .map(|a| AccountMetaData {
                    pubkey: a.pubkey,
                    is_signer: a.is_signer,
                    is_writable: a.is_writable,
                })
                .collect(),
            data: instruction.data,
        }
    }
}

pub fn print_governance_ix(ixs: &[Instruction]) {
    ixs.iter().for_each(|ix| {
        println!("\n------ GOV IX ------\n");

        // Convert the instruction to governance InstructionData format
        let gov_ix_data = InstructionData::from(ix.clone());

        let mut buffer = Cursor::new(Vec::new());
        match gov_ix_data.serialize(&mut buffer) {
            Ok(_) => {
                for account in gov_ix_data.accounts {
                    println!("Account: {:?}", account.pubkey);
                }
                println!("Data: {:?}", gov_ix_data.data);
                let base64_ix = BASE64_STANDARD.encode(buffer.into_inner());
                println!("Base64 InstructionData: {base64_ix:?}\n");
            }
            Err(err) => {
                println!("Failed to serialize InstructionData: {err}");
            }
        }
    });
}
