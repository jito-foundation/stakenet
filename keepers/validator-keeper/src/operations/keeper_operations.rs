use solana_metrics::datapoint_info;

#[derive(Clone)]
pub enum KeeperCreates {
    CreateValidatorHistory,
}

impl KeeperCreates {
    pub const LEN: usize = 1;

    pub fn emit(created_accounts_for_epoch: &[u64; KeeperCreates::LEN]) {
        let aggregate_creates = created_accounts_for_epoch.iter().sum::<u64>();

        datapoint_info!(
            "keeper-create-stats",
            // AGGREGATE
            ("num-aggregate-creates", aggregate_creates, i64),
            // CREATE VALIDATOR HISTORY
            (
                "num-validator-history-creates",
                created_accounts_for_epoch[KeeperCreates::CreateValidatorHistory as usize],
                i64
            ),
        );
    }
}

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
        txs_for_epoch: &[u64; KeeperOperations::LEN],
    ) {
        let aggregate_actions = runs_for_epoch.iter().sum::<u64>();
        let aggregate_errors = errors_for_epoch.iter().sum::<u64>();
        let aggregate_txs = txs_for_epoch.iter().sum::<u64>();

        datapoint_info!(
            "keeper-operation-stats",
            // AGGREGATE
            ("num-aggregate-actions", aggregate_actions, i64),
            ("num-aggregate-errors", aggregate_errors, i64),
            ("num-aggregate-txs", aggregate_txs, i64),
            // PRE CREATE UPDATE
            (
                "num-pre-create-update-runs",
                runs_for_epoch[KeeperOperations::PreCreateUpdate as usize],
                i64
            ),
            (
                "num-pre-create-update-errors",
                errors_for_epoch[KeeperOperations::PreCreateUpdate as usize],
                i64
            ),
            (
                "num-pre-create-update-txs",
                txs_for_epoch[KeeperOperations::PreCreateUpdate as usize],
                i64
            ),
            // CREATE MISSING ACCOUNTS
            (
                "num-create-missing-accounts-runs",
                runs_for_epoch[KeeperOperations::CreateMissingAccounts as usize],
                i64
            ),
            (
                "num-create-missing-accounts-errors",
                errors_for_epoch[KeeperOperations::CreateMissingAccounts as usize],
                i64
            ),
            (
                "num-create-missing-accounts-txs",
                txs_for_epoch[KeeperOperations::CreateMissingAccounts as usize],
                i64
            ),
            // POST CREATE UPDATE
            (
                "num-post-create-update-runs",
                runs_for_epoch[KeeperOperations::PostCreateUpdate as usize],
                i64
            ),
            (
                "num-post-create-update-errors",
                errors_for_epoch[KeeperOperations::PostCreateUpdate as usize],
                i64
            ),
            (
                "num-post-create-update-txs",
                txs_for_epoch[KeeperOperations::PostCreateUpdate as usize],
                i64
            ),
            // CLUSTER HISTORY
            (
                "num-cluster-history-runs",
                runs_for_epoch[KeeperOperations::ClusterHistory as usize],
                i64
            ),
            (
                "num-cluster-history-errors",
                errors_for_epoch[KeeperOperations::ClusterHistory as usize],
                i64
            ),
            (
                "num-cluster-history-txs",
                txs_for_epoch[KeeperOperations::ClusterHistory as usize],
                i64
            ),
            // GOSSIP UPLOAD
            (
                "num-gossip-upload-runs",
                runs_for_epoch[KeeperOperations::GossipUpload as usize],
                i64
            ),
            (
                "num-gossip-upload-errors",
                errors_for_epoch[KeeperOperations::GossipUpload as usize],
                i64
            ),
            (
                "num-gossip-upload-txs",
                txs_for_epoch[KeeperOperations::GossipUpload as usize],
                i64
            ),
            // STAKE UPLOAD
            (
                "num-stake-upload-runs",
                runs_for_epoch[KeeperOperations::StakeUpload as usize],
                i64
            ),
            (
                "num-stake-upload-errors",
                errors_for_epoch[KeeperOperations::StakeUpload as usize],
                i64
            ),
            (
                "num-stake-upload-txs",
                txs_for_epoch[KeeperOperations::StakeUpload as usize],
                i64
            ),
            // VOTE ACCOUNT
            (
                "num-vote-account-runs",
                runs_for_epoch[KeeperOperations::VoteAccount as usize],
                i64
            ),
            (
                "num-vote-account-errors",
                errors_for_epoch[KeeperOperations::VoteAccount as usize],
                i64
            ),
            (
                "num-vote-account-txs",
                txs_for_epoch[KeeperOperations::VoteAccount as usize],
                i64
            ),
            // MEV EARNED
            (
                "num-mev-earned-runs",
                runs_for_epoch[KeeperOperations::MevEarned as usize],
                i64
            ),
            (
                "num-mev-earned-errors",
                errors_for_epoch[KeeperOperations::MevEarned as usize],
                i64
            ),
            (
                "num-mev-earned-txs",
                txs_for_epoch[KeeperOperations::MevEarned as usize],
                i64
            ),
            // MEV COMMISSION
            (
                "num-mev-commission-runs",
                runs_for_epoch[KeeperOperations::MevCommission as usize],
                i64
            ),
            (
                "num-mev-commission-errors",
                errors_for_epoch[KeeperOperations::MevCommission as usize],
                i64
            ),
            (
                "num-mev-commission-txs",
                txs_for_epoch[KeeperOperations::MevCommission as usize],
                i64
            ),
            // EMIT METRICS
            (
                "num-emit-metrics-runs",
                runs_for_epoch[KeeperOperations::EmitMetrics as usize],
                i64
            ),
            (
                "num-emit-metrics-errors",
                errors_for_epoch[KeeperOperations::EmitMetrics as usize],
                i64
            ),
            (
                "num-emit-metrics-txs",
                txs_for_epoch[KeeperOperations::EmitMetrics as usize],
                i64
            ),
        );
    }
}
