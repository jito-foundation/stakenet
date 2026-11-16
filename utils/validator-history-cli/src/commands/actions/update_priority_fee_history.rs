use std::{path::PathBuf, sync::Arc};

use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use anyhow::anyhow;
use clap::Parser;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction, pubkey::Pubkey, signature::read_keypair_file, signer::Signer,
    transaction::Transaction,
};
use stakenet_sdk::utils::accounts::{
    get_validator_history_address, get_validator_history_config_address,
};
use validator_history::state::Config;

#[derive(Parser)]
#[command(about = "Updates priority fee and block metadata for a specific vote account")]
pub struct UpdatePriorityFeeHistory {
    /// Path to the priority fee oracle authority keypair, used to sign and pay for the transaction
    #[arg(short, long, env, default_value = "~/.config/solana/id.json")]
    keypair_path: PathBuf,

    /// Vote account to update priority fee history for
    #[arg(long, env)]
    vote_account: Pubkey,

    /// Epoch to write the entry for. Must not be in the future.
    #[arg(long, env)]
    epoch: u64,

    /// Total priority fees earned by the validator during the epoch, in lamports
    #[arg(long, env)]
    total_priority_fees: u64,

    /// Total number of leader slots assigned to the validator during the epoch
    #[arg(long, env)]
    total_leader_slots: u32,

    /// Number of blocks the validator actually produced during the epoch
    #[arg(long, env)]
    blocks_produced: u32,

    /// Highest slot the oracle has processed. Written to `block_data_updated_at_slot`.
    ///
    /// The keeper normally sets this to the highest globally observed finalized slot. Setting it
    /// past the last slot of `epoch` marks that epoch as final and stops the keeper from
    /// overwriting this entry.
    #[arg(long, env)]
    highest_oracle_recorded_slot: u64,
}

pub fn run(
    args: UpdatePriorityFeeHistory,
    client: RpcClient,
    program_id: Pubkey,
) -> anyhow::Result<()> {
    if args.blocks_produced > args.total_leader_slots {
        return Err(anyhow!(
            "blocks_produced ({}) cannot exceed total_leader_slots ({})",
            args.blocks_produced,
            args.total_leader_slots
        ));
    }

    let keypair = read_keypair_file(args.keypair_path)
        .map_err(|e| anyhow!("Failed reading keypair file: {e}"))?;
    let keypair = Arc::new(keypair);

    let config_address = get_validator_history_config_address(&program_id);
    let validator_history_account = get_validator_history_address(&args.vote_account, &program_id);

    // The program rejects future epochs with `EpochOutOfRange`; fail early with a clearer message.
    let epoch_info = client.get_epoch_info()?;
    if args.epoch > epoch_info.epoch {
        return Err(anyhow!(
            "Cannot write epoch {} because the current epoch is {}",
            args.epoch,
            epoch_info.epoch
        ));
    }

    if args.highest_oracle_recorded_slot > epoch_info.absolute_slot {
        return Err(anyhow!(
            "Highest oracle recorded slot {} cannot exceed current slot {}",
            args.highest_oracle_recorded_slot,
            epoch_info.absolute_slot
        ));
    }

    // `UpdatePriorityFeeHistory` has a `has_one = priority_fee_oracle_authority` constraint, so a
    // mismatched signer fails on-chain. Check it up front so the error is actionable.
    let config_account = client
        .get_account(&config_address)
        .map_err(|e| anyhow!("Failed fetching config account {config_address}: {e}"))?;
    let config = Config::try_deserialize(&mut config_account.data.as_slice())
        .map_err(|e| anyhow!("Failed deserializing config account {config_address}: {e}"))?;
    if config.priority_fee_oracle_authority != keypair.pubkey() {
        return Err(anyhow!(
            "Signer {} is not the priority fee oracle authority (expected {})",
            keypair.pubkey(),
            config.priority_fee_oracle_authority
        ));
    }

    println!("Validator history account: {validator_history_account}");
    println!("Vote account:              {}", args.vote_account);
    println!("Epoch:                     {}", args.epoch);
    println!("Total priority fees:       {}", args.total_priority_fees);
    println!("Total leader slots:        {}", args.total_leader_slots);
    println!("Blocks produced:           {}", args.blocks_produced);
    println!(
        "Block data updated at:     {}",
        args.highest_oracle_recorded_slot
    );

    let instruction = Instruction {
        program_id,
        accounts: validator_history::accounts::UpdatePriorityFeeHistory {
            validator_history_account,
            vote_account: args.vote_account,
            config: config_address,
            priority_fee_oracle_authority: keypair.pubkey(),
        }
        .to_account_metas(None),
        data: validator_history::instruction::UpdatePriorityFeeHistory {
            epoch: args.epoch,
            total_priority_fees: args.total_priority_fees,
            total_leader_slots: args.total_leader_slots,
            blocks_produced: args.blocks_produced,
            highest_oracle_recorded_slot: args.highest_oracle_recorded_slot,
        }
        .data(),
    };

    let hash = client
        .get_latest_blockhash()
        .map_err(|e| anyhow!("Failed to fetch latest blockhash: {e}"))?;
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&keypair.pubkey()),
        std::slice::from_ref(&keypair),
        hash,
    );
    let signature = client.send_transaction(&transaction)?;
    println!("Submit Result: {signature:?}");

    Ok(())
}
