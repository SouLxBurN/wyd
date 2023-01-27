use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChatMessage {
    pub client_id: String,
    pub message: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LocationMessage {
    pub client_id: String,
    pub location: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PresenceMessage {
    pub location: String,
    pub clients: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Payload {
    Chat(ChatMessage),
    Location(LocationMessage),
    Presence(PresenceMessage),
}
