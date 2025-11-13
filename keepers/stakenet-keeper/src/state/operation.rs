use crate::operations::keeper_operations::KeeperOperations;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationState {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalType {
    ValidatorHistory,
    Steward,
    BlockMetadata,
    Metrics,
}

#[derive(Debug, Clone)]
pub struct OperationTask {
    pub operation: KeeperOperations,
    pub state: OperationState,
    pub interval_type: IntervalType,
}

pub struct OperationQueue {
    pub tasks: Vec<OperationTask>,
    pub current_index: usize,
    pub validator_history_interval: u64,
    pub steward_interval: u64,
    pub block_metadata_interval: u64,
    pub metrics_interval: u64,
}

impl OperationQueue {
    pub fn new(
        validator_history_interval: u64,
        steward_interval: u64,
        block_metadata_interval: u64,
        metrics_interval: u64,
        run_flags: u32,
    ) -> Self {
        let mut tasks = Vec::new();

        // Build tasks in execution order based on run_flags

        // Fetch operations (validator_history_interval)
        if run_flags & (1 << KeeperOperations::PreCreateUpdate as u32) != 0 {
            tasks.push(OperationTask {
                operation: KeeperOperations::PreCreateUpdate,
                state: OperationState::Pending,
                interval_type: IntervalType::ValidatorHistory,
            });
        }

        if run_flags & (1 << KeeperOperations::CreateMissingAccounts as u32) != 0 {
            tasks.push(OperationTask {
                operation: KeeperOperations::CreateMissingAccounts,
                state: OperationState::Pending,
                interval_type: IntervalType::ValidatorHistory,
            });
        }

        if run_flags & (1 << KeeperOperations::PostCreateUpdate as u32) != 0 {
            tasks.push(OperationTask {
                operation: KeeperOperations::PostCreateUpdate,
                state: OperationState::Pending,
                interval_type: IntervalType::ValidatorHistory,
            });
        }

        // Validator history operations
        if run_flags & (1 << KeeperOperations::ClusterHistory as u32) != 0 {
            tasks.push(OperationTask {
                operation: KeeperOperations::ClusterHistory,
                state: OperationState::Pending,
                interval_type: IntervalType::ValidatorHistory,
            });
        }

        if run_flags & (1 << KeeperOperations::VoteAccount as u32) != 0 {
            tasks.push(OperationTask {
                operation: KeeperOperations::VoteAccount,
                state: OperationState::Pending,
                interval_type: IntervalType::ValidatorHistory,
            });
        }

        if run_flags & (1 << KeeperOperations::MevCommission as u32) != 0 {
            tasks.push(OperationTask {
                operation: KeeperOperations::MevCommission,
                state: OperationState::Pending,
                interval_type: IntervalType::ValidatorHistory,
            });
        }

        if run_flags & (1 << KeeperOperations::MevEarned as u32) != 0 {
            tasks.push(OperationTask {
                operation: KeeperOperations::MevEarned,
                state: OperationState::Pending,
                interval_type: IntervalType::ValidatorHistory,
            });
        }

        if run_flags & (1 << KeeperOperations::StakeUpload as u32) != 0 {
            tasks.push(OperationTask {
                operation: KeeperOperations::StakeUpload,
                state: OperationState::Pending,
                interval_type: IntervalType::ValidatorHistory,
            });
        }

        if run_flags & (1 << KeeperOperations::GossipUpload as u32) != 0 {
            tasks.push(OperationTask {
                operation: KeeperOperations::GossipUpload,
                state: OperationState::Pending,
                interval_type: IntervalType::ValidatorHistory,
            });
        }

        if run_flags & (1 << KeeperOperations::PriorityFeeCommission as u32) != 0 {
            tasks.push(OperationTask {
                operation: KeeperOperations::PriorityFeeCommission,
                state: OperationState::Pending,
                interval_type: IntervalType::ValidatorHistory,
            });
        }

        // Steward operation
        if run_flags & (1 << KeeperOperations::Steward as u32) != 0 {
            tasks.push(OperationTask {
                operation: KeeperOperations::Steward,
                state: OperationState::Pending,
                interval_type: IntervalType::Steward,
            });
        }

        // Block metadata operation
        if run_flags & (1 << KeeperOperations::BlockMetadataKeeper as u32) != 0 {
            tasks.push(OperationTask {
                operation: KeeperOperations::BlockMetadataKeeper,
                state: OperationState::Pending,
                interval_type: IntervalType::BlockMetadata,
            });
        }

        // Metrics operation
        if run_flags & (1 << KeeperOperations::EmitMetrics as u32) != 0 {
            tasks.push(OperationTask {
                operation: KeeperOperations::EmitMetrics,
                state: OperationState::Pending,
                interval_type: IntervalType::Metrics,
            });
        }

        Self {
            tasks,
            current_index: 0,
            validator_history_interval,
            steward_interval,
            block_metadata_interval,
            metrics_interval,
        }
    }

    fn get_all_intervals(&self) -> Vec<u64> {
        vec![
            self.validator_history_interval,
            self.metrics_interval,
            self.steward_interval,
            self.block_metadata_interval,
        ]
    }

    fn should_update(&self, tick: u64) -> bool {
        self.get_all_intervals()
            .iter()
            .any(|interval| tick % interval == 0)
    }

    fn should_emit(&self, tick: u64) -> bool {
        self.get_all_intervals()
            .iter()
            .any(|interval| tick % (interval + 1) == 0)
    }

    pub fn mark_should_fire(&mut self, tick: u64) {
        let should_update = self.should_update(tick);
        let should_emit = self.should_emit(tick);

        for task in self.tasks.iter_mut() {
            let should_fire = match task.operation {
                // Fetch operations use should_update logic
                KeeperOperations::PreCreateUpdate
                | KeeperOperations::CreateMissingAccounts
                | KeeperOperations::PostCreateUpdate => should_update,

                // Metrics uses should_emit logic
                KeeperOperations::EmitMetrics => should_emit,

                // All other operations use standard interval check
                _ => {
                    let interval = match task.interval_type {
                        IntervalType::ValidatorHistory => self.validator_history_interval,
                        IntervalType::Steward => self.steward_interval,
                        IntervalType::BlockMetadata => self.block_metadata_interval,
                        IntervalType::Metrics => self.metrics_interval,
                    };
                    tick % interval == 0
                }
            };

            task.state = if should_fire {
                OperationState::Pending
            } else {
                OperationState::Skipped
            };
        }
    }

    pub fn get_next_pending(&mut self) -> Option<&mut OperationTask> {
        for i in self.current_index..self.tasks.len() {
            if self.tasks[i].state == OperationState::Pending {
                self.current_index = i;
                return Some(&mut self.tasks[i]);
            }
        }
        None
    }

    pub fn mark_completed(&mut self, operation: KeeperOperations) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.operation == operation) {
            task.state = OperationState::Completed;
        }
    }

    pub fn mark_failed(&mut self, operation: KeeperOperations) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.operation == operation) {
            task.state = OperationState::Failed;
        }
    }

    pub fn reset_for_next_cycle(&mut self) {
        self.current_index = 0;
        // States will be set by mark_should_fire
    }
}
