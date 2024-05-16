use solana_metrics::datapoint_info;

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

    pub fn emit(
        runs_for_epoch: &[u64; KeeperOperations::LEN],
        errors_for_epoch: &[u64; KeeperOperations::LEN],
    ) {
        datapoint_info!(
            "keeper-operation-stats",
            (
                "num-pre-create-update-runs",
                runs_for_epoch[KeeperOperations::PreCreateUpdate as usize],
                i64
            ),
            (
                "num-pre-create-update-errors",
                errors_for_epoch[KeeperOperations::PreCreateUpdate as usize],
                i64
            )
        );
    }
}
