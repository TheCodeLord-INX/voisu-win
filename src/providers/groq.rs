//! Groq Whisper Large v3 REST LPU client.

pub struct GroqClient {
    api_key: String,
}

impl GroqClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
        }
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }
}
