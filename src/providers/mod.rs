pub mod coordinator;
pub mod deepgram;
pub mod groq;

pub use coordinator::{CoordinatorError, DualProviderCoordinator, DualProviderResult};
