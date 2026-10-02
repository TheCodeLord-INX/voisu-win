//! Deepgram Nova-2 streaming WebSocket client.

pub struct DeepgramClient {
    api_key: String,
}

impl DeepgramClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
        }
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }
}
