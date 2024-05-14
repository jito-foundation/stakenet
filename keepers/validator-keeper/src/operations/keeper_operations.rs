use solana_sdk::epoch_info::EpochInfo;

#[derive(Clone)]
pub enum KeeperOperations {
    UpdateEpoch,
    CreateValidatorHistory,
    ClusterHistory,
    GossipUpload,
    StakeUpload,
    VoteAccount,
    MevEarned,
    MevCommission,
    EmitMetrics,
}
impl KeeperOperations {
    pub const LEN: usize = 9;
}

pub trait KeeperOperation {
    fn should_run(&self, epoch_info: &EpochInfo) -> bool;
    fn send_and_emit(&self);
    fn emit_datapoints(&self);
}

// Test Operation
#[derive(Clone)]

pub struct ClusterHistoryOperation {
    pub runs_for_epoch: u64,
    pub errors_for_epoch: u64,
}
impl KeeperOperation for ClusterHistoryOperation {
    fn should_run(&self, epoch_info: &EpochInfo) -> bool {
        let runs_to_check = self.runs_for_epoch;

        (epoch_info.slot_index > epoch_info.slots_in_epoch / 1000 && runs_to_check < 1)
            || (epoch_info.slot_index > epoch_info.slots_in_epoch / 2 && runs_to_check < 2)
            || (epoch_info.slot_index > epoch_info.slots_in_epoch * 9 / 10 && runs_to_check < 3)
    }
    fn send_and_emit(&self) {
        // Send and Emit
    }
    fn emit_datapoints(&self) {
        // Emit Datapoints
    }
}
