use super::errors::JitoSendTransactionError;

#[derive(Debug, Default, Clone)]
pub struct SubmitStats {
    pub successes: u64,
    pub errors: u64,
    pub results: Vec<Result<(), JitoSendTransactionError>>,
}

impl SubmitStats {
    pub fn combine(&mut self, other: &SubmitStats) {
        self.successes += other.successes;
        self.errors += other.errors;
        self.results.extend(other.results.clone())
    }
}
#[derive(Debug, Default, Clone)]
pub struct CreateUpdateStats {
    pub creates: SubmitStats,
    pub updates: SubmitStats,
}
