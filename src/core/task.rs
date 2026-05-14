use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Task {
    pub id: String,
    pub command: String,
    pub owner: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed(EncryptedResult),
    Failed(String),
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct TaskResult {
    pub task_id: String,
    pub output: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct EncryptedResult {
    pub task_id: String,
    pub data: Vec<u8>,
    pub nonce: Vec<u8>,
}
