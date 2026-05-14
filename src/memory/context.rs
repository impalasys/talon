use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryContext {
    pub long_term: String,
    pub thread_log: String,
    pub daily_log: String,
    pub workspace_id: String,
    pub thread_id: String,
}

impl MemoryContext {
    pub fn prune(&self, max_chars: usize) -> (String, String, String) {
        let remaining = max_chars;
        let thread_budget = remaining / 2;
        let thread_log = truncate(&self.thread_log, thread_budget);
        let lt_budget = (remaining - thread_log.len()) * 7 / 10;
        let long_term = truncate(&self.long_term, lt_budget);
        let daily_budget = remaining - thread_log.len() - long_term.len();
        let daily_log = truncate(&self.daily_log, daily_budget);
        (long_term, thread_log, daily_log)
    }
}

pub fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    if max_len < 4 {
        return "".to_string();
    }
    format!("...\n{}", &s[s.len() - max_len + 4..])
}
