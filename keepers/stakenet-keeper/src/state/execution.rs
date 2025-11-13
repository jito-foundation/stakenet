#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionBlock {
    Fetch,            // Pre-create, create missing, post-create
    ValidatorHistory, // Cluster history, vote accounts, MEV, etc.
    Steward,          // Steward operations
    BlockMetadata,    // Priority fee block metadata
    MetricsEmit,      // Emit metrics
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiringCondition {
    Standard,    // tick % interval == 0
    EmitStyle,   // tick % (interval + 1) == 0
    UpdateStyle, // tick % interval == 0, but check against multiple intervals
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockState {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone)]
pub struct ExecutionTask {
    pub block: ExecutionBlock,
    pub state: BlockState,
    pub interval: u64,
    pub firing_condition: FiringCondition,
}

#[derive(Default)]
pub struct ExecutionQueue {
    pub tasks: Vec<ExecutionTask>,
    pub current_index: usize,
    pub intervals: Vec<u64>,
}

impl ExecutionQueue {
    pub fn new(
        validator_history_interval: u64,
        steward_interval: u64,
        block_metadata_interval: u64,
        metrics_interval: u64,
        _run_flags: u32,
    ) -> Self {
        let intervals = vec![
            validator_history_interval,
            metrics_interval,
            steward_interval,
            block_metadata_interval,
        ];

        let tasks = vec![
            // Fetch block - always runs when validator_history_interval fires
            ExecutionTask {
                block: ExecutionBlock::Fetch,
                state: BlockState::Pending,
                interval: validator_history_interval,
                firing_condition: FiringCondition::UpdateStyle,
            },
            // Validator History block
            ExecutionTask {
                block: ExecutionBlock::ValidatorHistory,
                state: BlockState::Pending,
                interval: validator_history_interval,
                firing_condition: FiringCondition::Standard,
            },
            // Steward block
            ExecutionTask {
                block: ExecutionBlock::Steward,
                state: BlockState::Pending,
                interval: steward_interval,
                firing_condition: FiringCondition::Standard,
            },
            // Block Metadata block
            ExecutionTask {
                block: ExecutionBlock::BlockMetadata,
                state: BlockState::Pending,
                interval: block_metadata_interval,
                firing_condition: FiringCondition::Standard,
            },
            // Metrics Emit block
            ExecutionTask {
                block: ExecutionBlock::MetricsEmit,
                state: BlockState::Pending,
                interval: metrics_interval,
                firing_condition: FiringCondition::EmitStyle,
            },
        ];

        Self {
            tasks,
            current_index: 0,
            intervals,
        }
    }

    pub fn mark_should_fire(&mut self, tick: u64) {
        for task in &mut self.tasks {
            let should_fire = match task.firing_condition {
                FiringCondition::Standard => {
                    // tick % interval == 0
                    tick % task.interval == 0
                }
                FiringCondition::EmitStyle => {
                    // tick % (interval + 1) == 0
                    // OR any interval from the intervals array matches
                    self.intervals
                        .iter()
                        .any(|interval| tick % (interval + 1) == 0)
                }
                FiringCondition::UpdateStyle => {
                    // Any interval matches: tick % interval == 0
                    self.intervals.iter().any(|interval| tick % interval == 0)
                }
            };

            task.state = if should_fire {
                BlockState::Pending
            } else {
                BlockState::Skipped
            };
        }
    }

    pub fn get_next_pending(&mut self) -> Option<&mut ExecutionTask> {
        for i in self.current_index..self.tasks.len() {
            if self.tasks[i].state == BlockState::Pending {
                self.current_index = i;
                return Some(&mut self.tasks[i]);
            }
        }
        None
    }

    pub fn mark_completed(&mut self, block: ExecutionBlock) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.block == block) {
            task.state = BlockState::Completed;
        }
    }

    pub fn mark_failed(&mut self, block: ExecutionBlock) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.block == block) {
            task.state = BlockState::Failed;
        }
    }

    // pub fn reset_for_next_cycle(&mut self) {
    //     self.current_index = 0;
    //     // Don't reset state here - it gets set by mark_should_fire
    // }

    // pub fn all_completed(&self) -> bool {
    //     self.tasks
    //         .iter()
    //         .filter(|t| t.state == BlockState::Pending || t.state == BlockState::Running)
    //         .count()
    //         == 0
    // }
}
