#[derive(Clone)]
pub enum KeeperOperations {
    UpdateState,
    ClusterHistory,
    GossipUpload,
    StakeUpload,
    VoteAccount,
    MevEarned,
    MevCommission,
    EmitMetrics,
}
impl KeeperOperations {
    pub const LEN: usize = 8;
}
