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
