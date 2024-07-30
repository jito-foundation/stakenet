use jito_steward::{
    StewardState, COMPUTE_DELEGATIONS, COMPUTE_INSTANT_UNSTAKES, COMPUTE_SCORE, EPOCH_MAINTENANCE,
    POST_LOOP_IDLE, PRE_LOOP_IDLE, REBALANCE,
};

// ----------- STEWARD STATE --------------

pub enum StateCode {
    NoState = 0x00,
    ComputeScore = 0x01 << 0,
    ComputeDelegations = 0x01 << 1,
    PreLoopIdle = 0x01 << 2,
    ComputeInstantUnstake = 0x01 << 3,
    Rebalance = 0x01 << 4,
    PostLoopIdle = 0x01 << 5,
}

pub fn steward_state_to_state_code(steward_state: &StewardState) -> StateCode {
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
    } else {
        StateCode::NoState
    }
}

pub fn format_steward_state_string(steward_state: &StewardState) -> String {
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

    if steward_state.has_flag(POST_LOOP_IDLE) {
        state_string += "▣"
    } else {
        state_string += "□"
    }

    state_string
}

pub fn format_simple_steward_state_string(steward_state: &StewardState) -> String {
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

    if steward_state.has_flag(POST_LOOP_IDLE) {
        state_string += "1"
    } else {
        state_string += "-"
    }

    state_string
}
