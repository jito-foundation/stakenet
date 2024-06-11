use std::sync::Arc;

use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, signature::Keypair, signer::Signer,
    transaction::Transaction,
};

pub fn package_instructions(
    ixs: &Vec<Instruction>,
    chunk_size: usize,
    priority_fee: Option<u64>,
    compute_limit: Option<u32>,
    hash_size: Option<u32>,
) -> Vec<Vec<Instruction>> {
    ixs.chunks(chunk_size)
        .map(|chunk: &[Instruction]| {
            let mut chunk_vec = chunk.to_vec();
            if let Some(compute_limit) = compute_limit {
                chunk_vec.insert(
                    0,
                    ComputeBudgetInstruction::set_compute_unit_limit(compute_limit),
                );
            }
            if let Some(priority_fee) = priority_fee {
                chunk_vec.insert(
                    0,
                    ComputeBudgetInstruction::set_compute_unit_price(priority_fee),
                );
            }
            if let Some(hash_size) = hash_size {
                chunk_vec.insert(0, ComputeBudgetInstruction::request_heap_frame(hash_size));
            }

            chunk_vec
        })
        .collect::<Vec<Vec<Instruction>>>()
}

pub async fn debug_send_single_transaction(
    client: &Arc<RpcClient>,
    payer: &Arc<Keypair>,
    instructions: &Vec<Instruction>,
    debug_print: Option<bool>,
) -> Result<solana_sdk::signature::Signature, solana_client::client_error::ClientError> {
    let transaction = Transaction::new_signed_with_payer(
        &instructions,
        Some(&payer.pubkey()),
        &[&payer],
        client.get_latest_blockhash().await?,
    );

    let result = client.send_and_confirm_transaction(&transaction).await;

    if let Some(debug_print) = debug_print {
        match &result {
            Ok(signature) => {
                println!("Signature: {}", signature);
            }
            Err(e) => {
                println!("Accounts: {:?}", &instructions.last().unwrap().accounts);
                println!("Error: {:?}", e);
            }
        }
    }

    return result;
}
