use std::sync::Arc;

use anchor_lang::{InstructionData, ToAccountMetas};
use anyhow::Result;
use jito_steward::StewardStateEnum;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_program::instruction::Instruction;

use solana_sdk::{
    pubkey::Pubkey, signature::read_keypair_file, signer::Signer, transaction::Transaction,
};

use crate::{
    commands::command_args::{CrankComputeDelegations, CrankMonkey},
    utils::{
        accounts::{get_all_steward_accounts, get_steward_state_account},
        transactions::configure_instruction,
    },
};

// Only runs one set of commands per "crank"

pub async fn command_crank_monkey(
    args: CrankMonkey,
    client: &Arc<RpcClient>,
    program_id: Pubkey,
) -> Result<()> {
    let steward_config = args.permissionless_parameters.steward_config;
    let all_steward_accounts =
        get_all_steward_accounts(client, &program_id, &steward_config).await?;

    // Check Epoch
    let epoch = client.get_epoch_info().await?.epoch;
    if all_steward_accounts.state_account.state.current_epoch != epoch {
        todo!("Check for indexes to remove");

        todo!("Call crank_epoch_maintenance");
        return Ok(());
    }

    // State
    match all_steward_accounts.state_account.state.state_tag {
        StewardStateEnum::ComputeScores => todo!("ComputeScores"),
        StewardStateEnum::ComputeDelegations => todo!("ComputeDelegations"),
        StewardStateEnum::Idle => todo!("Idle"),
        StewardStateEnum::ComputeInstantUnstake => todo!("ComputeInstantUnstake"),
        StewardStateEnum::Rebalance => todo!("Rebalance"),
    }

    // Rebound from any errors

    Ok(())
}
