//! Operation Queue Management
//!
//! This module provides a queue-based system for managing and executing keeper operations
//! in a controlled, sequential manner with support for interval-based scheduling and
//! epoch transition detection.
//!
//! # Architecture
//!
//! The queue system operates on a tick-based cycle (1 second per tick):
//! 1. **Mark Phase**: `mark_should_fire()` determines which operations should run based on their intervals
//! 2. **Execute Phase**: Operations are executed one-by-one via `get_next_pending()`
//! 3. **State Update**: After execution, operations are marked as Completed/Failed
//! 4. **Reset Phase**: Queue resets for the next cycle
//!
//! # Operation States
//!
//! - `Pending`: Operation should run this cycle and hasn't been executed yet
//! - `Completed`: Operation executed successfully this cycle
//! - `Failed`: Operation failed during execution
//! - `Skipped`: Operation should not run this cycle (based on interval)

use crate::operations::keeper_operations::KeeperOperations;

/// Represents the current execution state of an operation task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationState {
    /// Operation should execute this cycle and hasn't been executed yet
    Pending,

    /// Operation executed successfully in this cycle
    Completed,

    /// Operation failed during execution in this cycle
    Failed,

    /// Operation should not execute this cycle based on interval check
    Skipped,
}

/// Defines the interval category for an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervalType {
    /// Operations that fire at `validator_history_interval`
    /// (e.g., ClusterHistory, VoteAccount, MEV operations)
    ValidatorHistory,

    /// Operations that fire at `steward_interval`
    /// (e.g., Steward cranking operations)
    Steward,

    /// Operations that fire at `block_metadata_interval`
    /// (e.g., Priority fee block metadata)
    BlockMetadata,

    /// Operations that fire at `metrics_interval`
    /// (e.g., Metrics emission)
    Metrics,
}

/// Represents a single operation task in the execution queue.
#[derive(Debug, Clone)]
pub struct OperationTask {
    /// The keeper operation to execute
    pub operation: KeeperOperations,

    /// Current state of this task in the execution cycle
    pub state: OperationState,

    /// Interval category that determines when this operation fires
    pub interval_type: IntervalType,
}

/// Queue for managing keeper operations execution order and state.
pub struct OperationQueue {
    /// List of all operation tasks in execution order
    pub tasks: Vec<OperationTask>,

    /// Index of the next task to check in the current cycle.
    current_index: usize,

    /// Interval in seconds for validator history operations
    validator_history_interval: u64,

    /// Interval in seconds for steward operations
    steward_interval: u64,

    /// Interval in seconds for block metadata operations
    block_metadata_interval: u64,

    /// Interval in seconds for metrics operations
    metrics_interval: u64,
}

impl OperationQueue {
    /// Creates a new operation queue based on run flags.
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

    /// Returns all configured intervals
    fn get_all_intervals(&self) -> Vec<u64> {
        vec![
            self.validator_history_interval,
            self.metrics_interval,
            self.steward_interval,
            self.block_metadata_interval,
        ]
    }

    /// Checks if any interval matches for update operations.
    fn should_update(&self, tick: u64) -> bool {
        self.get_all_intervals()
            .iter()
            .any(|interval| tick % interval == 0)
    }

    /// Checks if any interval matches for emit operations.
    fn should_emit(&self, tick: u64) -> bool {
        self.get_all_intervals()
            .iter()
            .any(|interval| tick % (interval + 1) == 0)
    }

    /// Marks which operations should fire this cycle based on current tick and their intervals.
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

    /// Returns the next pending task in the queue.
    pub fn get_next_pending(&mut self) -> Option<&mut OperationTask> {
        for i in self.current_index..self.tasks.len() {
            if self.tasks[i].state == OperationState::Pending {
                self.current_index = i;
                return Some(&mut self.tasks[i]);
            }
        }
        None
    }

    /// Marks an operation as successfully completed.
    pub fn mark_completed(&mut self, operation: KeeperOperations) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.operation == operation) {
            task.state = OperationState::Completed;
        }
    }

    /// Marks an operation as failed.
    pub fn mark_failed(&mut self, operation: KeeperOperations) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.operation == operation) {
            task.state = OperationState::Failed;
        }
    }

    /// Resets the queue for the next execution cycle.
    pub fn reset_for_next_cycle(&mut self) {
        self.current_index = 0;
    }
}
