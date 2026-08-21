use bevy::prelude::*;
use std::sync::{Arc, Mutex};
use tracing_subscriber::Layer;

/// A tracing layer that captures log messages into a ring buffer.
///
/// Messages are stored in a shared buffer that can be queried by the MCP
/// `logs` tool. The buffer has a fixed capacity and old messages are dropped
/// when it fills up.
#[derive(Resource, Clone)]
pub struct LogCapture {
    messages: Arc<Mutex<Vec<LogEntry>>>,
    max_entries: usize,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: String,
    pub message: String,
    pub target: String,
    pub timestamp: String,
}

impl LogCapture {
    pub fn new(max_entries: usize) -> Self {
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
            max_entries,
        }
    }

    pub fn get_entries(&self, level_filter: Option<&str>, limit: usize) -> Vec<LogEntry> {
        let messages = self.messages.lock().unwrap();
        let filtered: Vec<LogEntry> = if let Some(level) = level_filter {
            messages
                .iter()
                .filter(|entry| entry.level.to_lowercase() == level.to_lowercase())
                .cloned()
                .collect()
        } else {
            messages.clone()
        };
        filtered.into_iter().rev().take(limit).collect()
    }

    pub fn layer(&self) -> LogCaptureLayer {
        LogCaptureLayer {
            messages: self.messages.clone(),
            max_entries: self.max_entries,
        }
    }
}

pub struct LogCaptureLayer {
    messages: Arc<Mutex<Vec<LogEntry>>>,
    max_entries: usize,
}

impl<S> Layer<S> for LogCaptureLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        let level = metadata.level().to_string();
        let target = metadata.target().to_string();

        // Extract the message from the event fields.
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let message = visitor.message.unwrap_or_default();

        let entry = LogEntry {
            level,
            message,
            target,
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        let mut messages = self.messages.lock().unwrap();
        messages.push(entry);
        if messages.len() > self.max_entries {
            messages.remove(0);
        }
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: Option<String>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        }
    }
}
