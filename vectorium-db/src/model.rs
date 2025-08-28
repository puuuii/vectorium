use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Document {
    pub id: String, // UUID or hash of the file path
    pub file_path: String,
    pub content: String,
    pub last_modified: std::time::SystemTime,
}