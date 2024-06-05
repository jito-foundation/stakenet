use anchor_lang::AccountDeserialize;
use jito_steward::{StewardStateAccount, StewardStateEnum};
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;

use super::commands::ViewState;

pub fn command_view_state(args: ViewState, client: RpcClient, program_id: Pubkey) {
    let steward_config = args.steward_config;

    let (steward_state, _) = Pubkey::find_program_address(
        &[StewardStateAccount::SEED, steward_config.as_ref()],
        &program_id,
    );

    let state_raw_account = client
        .get_account(&steward_state)
        .expect("Cannot find state account");

    let state_account: StewardStateAccount =
        StewardStateAccount::try_deserialize(&mut state_raw_account.data.as_slice())
            .expect("Cannot deserialize state account");

    // let mut output = String::new(); // Initialize the string directly
    let output = _print_default_state(&steward_config, &steward_state, &state_account).to_string();

    println!("{}", output);
}

fn _state_tag_to_string(tag: StewardStateEnum) -> String {
    match tag {
        StewardStateEnum::ComputeScores => "Compute Scores".to_string(),
        StewardStateEnum::ComputeDelegations => "Compute Delegations".to_string(),
        StewardStateEnum::Idle => "Idle".to_string(),
        StewardStateEnum::ComputeInstantUnstake => "Compute Instant Unstake".to_string(),
        StewardStateEnum::Rebalance => "Rebalance".to_string(),
    }
}

fn _print_default_state(
    steward_config: &Pubkey,
    steward_state: &Pubkey,
    state_account: &StewardStateAccount,
) -> String {
    let state = &state_account.state;

    let mut formatted_string = String::new();

    formatted_string += "------- State -------\n";
    formatted_string += "📚 Accounts 📚\n";
    formatted_string += &format!("Config:      {}\n", steward_config);
    formatted_string += &format!("State:       {}\n", steward_state);
    formatted_string += "\n";
    formatted_string += "↺ State ↺\n";
    formatted_string += &format!("State Tag: {:?}\n", _state_tag_to_string(state.state_tag));
    formatted_string += &format!(
        "Validator Lamport Balances Count: {}\n",
        state.validator_lamport_balances.len()
    );
    formatted_string += &format!("Scores Count: {}\n", state.scores.len());
    formatted_string += &format!(
        "Sorted Score Indices Count: {}\n",
        state.sorted_score_indices.len()
    );
    formatted_string += &format!("Yield Scores Count: {}\n", state.yield_scores.len());
    formatted_string += &format!(
        "Sorted Yield Score Indices Count: {}\n",
        state.sorted_yield_score_indices.len()
    );
    formatted_string += &format!("Delegations Count: {}\n", state.delegations.len());
    formatted_string += &format!("Instant Unstake: {:?}\n", state.instant_unstake.count());
    formatted_string += &format!("Progress: {:?}\n", state.progress.count());
    formatted_string += &format!(
        "Start Computing Scores Slot: {}\n",
        state.start_computing_scores_slot
    );
    formatted_string += &format!("Current Epoch: {}\n", state.current_epoch);
    formatted_string += &format!("Next Cycle Epoch: {}\n", state.next_cycle_epoch);
    formatted_string += &format!("Number of Pool Validators: {}\n", state.num_pool_validators);
    formatted_string += &format!("Scoring Unstake Total: {}\n", state.scoring_unstake_total);
    formatted_string += &format!("Instant Unstake Total: {}\n", state.instant_unstake_total);
    formatted_string += &format!(
        "Stake Deposit Unstake Total: {}\n",
        state.stake_deposit_unstake_total
    );
    formatted_string += &format!(
        "Compute Delegations Completed: {:?}\n",
        state.compute_delegations_completed
    );
    formatted_string += &format!("Rebalance Completed: {:?}\n", state.rebalance_completed);
    formatted_string += &format!("Padding0 Length: {}\n", state._padding0.len());
    formatted_string += "---------------------";

    formatted_string
}
