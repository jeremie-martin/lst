use std::sync::Arc;

pub trait Clock: Send + Sync {
    fn scratchpad_timestamp(&self) -> String;
}

pub type SharedClock = Arc<dyn Clock>;

pub struct RealClock;

impl Clock for RealClock {
    fn scratchpad_timestamp(&self) -> String {
        use chrono::Local;
        Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
    }
}

pub struct FixedClock {
    timestamp: String,
}

impl FixedClock {
    pub fn new(timestamp: impl Into<String>) -> Self {
        Self {
            timestamp: timestamp.into(),
        }
    }
}

impl Default for FixedClock {
    fn default() -> Self {
        Self::new("1970-01-01_00-00-00")
    }
}

impl Clock for FixedClock {
    fn scratchpad_timestamp(&self) -> String {
        self.timestamp.clone()
    }
}
