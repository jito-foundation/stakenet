use std::sync::Arc;

use jito_steward::{
    StewardStateV2, COMPUTE_DELEGATIONS, COMPUTE_INSTANT_UNSTAKES, COMPUTE_SCORE,
    EPOCH_MAINTENANCE, POST_LOOP_IDLE, PRE_LOOP_IDLE, REBALANCE, REBALANCE_DIRECTED_COMPLETE,
};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::Instruction, signature::Keypair, signer::Signer, transaction::Transaction,
};

// ----------- DEBUG SEND --------------

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

// ----------- STEWARD STATE --------------

pub enum StateCode {
    NoState = 0x00,
    ComputeScore = 0x01 << 0,
    ComputeDelegations = 0x01 << 1,
    PreLoopIdle = 0x01 << 2,
    ComputeInstantUnstake = 0x01 << 3,
    Rebalance = 0x01 << 4,
    PostLoopIdle = 0x01 << 5,
    RebalanceDirectedComplete = 0x01 << 6,
}

pub fn steward_state_to_state_code(steward_state: &StewardStateV2) -> StateCode {
    if steward_state.has_flag(POST_LOOP_IDLE) {
        StateCode::PostLoopIdle
    } else if steward_state.has_flag(REBALANCE) {
        StateCode::Rebalance
    } else if steward_state.has_flag(COMPUTE_INSTANT_UNSTAKES) {
        StateCode::ComputeInstantUnstake
    } else if steward_state.has_flag(PRE_LOOP_IDLE) {
        StateCode::PreLoopIdle
    } else if steward_state.has_flag(COMPUTE_DELEGATIONS) {
        StateCode::ComputeDelegations
    } else if steward_state.has_flag(COMPUTE_SCORE) {
        StateCode::ComputeScore
    } else if steward_state.has_flag(REBALANCE_DIRECTED_COMPLETE) {
        StateCode::RebalanceDirectedComplete
    } else {
        StateCode::NoState
    }
}

pub fn format_steward_state_string(steward_state: &StewardStateV2) -> String {
    let mut state_string = String::new();

    if steward_state.has_flag(EPOCH_MAINTENANCE) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    state_string += " ⇢ ";

    if steward_state.has_flag(COMPUTE_SCORE) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    if steward_state.has_flag(COMPUTE_DELEGATIONS) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    state_string += " ↺ ";

    if steward_state.has_flag(PRE_LOOP_IDLE) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    if steward_state.has_flag(COMPUTE_INSTANT_UNSTAKES) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    if steward_state.has_flag(REBALANCE) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    if steward_state.has_flag(REBALANCE_DIRECTED_COMPLETE) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    if steward_state.has_flag(POST_LOOP_IDLE) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    state_string
}

pub fn format_simple_steward_state_string(steward_state: &StewardStateV2) -> String {
    let mut state_string = String::new();

    if steward_state.has_flag(EPOCH_MAINTENANCE) {
        state_string += "M"
    } else {
        state_string += "-"
    }

    if steward_state.has_flag(COMPUTE_SCORE) {
        state_string += "S"
    } else {
        state_string += "-"
    }

    if steward_state.has_flag(COMPUTE_DELEGATIONS) {
        state_string += "D"
    } else {
        state_string += "-"
    }

    if steward_state.has_flag(PRE_LOOP_IDLE) {
        state_string += "0"
    } else {
        state_string += "-"
    }

    if steward_state.has_flag(COMPUTE_INSTANT_UNSTAKES) {
        state_string += "U"
    } else {
        state_string += "-"
    }

    if steward_state.has_flag(REBALANCE) {
        state_string += "R"
    } else {
        state_string += "-"
    }

    if steward_state.has_flag(REBALANCE_DIRECTED_COMPLETE) {
        state_string += "C"
    } else {
        state_string += "-"
    }

    if steward_state.has_flag(POST_LOOP_IDLE) {
        state_string += "1"
    } else {
        state_string += "-"
    }

    state_string
}
