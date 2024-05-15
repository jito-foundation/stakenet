#[derive(Clone)]
pub enum KeeperOperations {
    PreCreateUpdate,
    CreateMissingAccounts,
    PostCreateUpdate,
    ClusterHistory,
    GossipUpload,
    StakeUpload,
    VoteAccount,
    MevEarned,
    MevCommission,
    EmitMetrics,
}
impl KeeperOperations {
    pub const LEN: usize = 10;
}
