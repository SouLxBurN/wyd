use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub client_id: String,
    pub message: String,
    pub loc: Option<String>,
}

