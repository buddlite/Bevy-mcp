use bevy::prelude::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Resource that stores captured events for MCP observation.
///
/// Events are stored in a ring buffer with a fixed capacity.
/// Old events are dropped when the buffer fills up.
#[derive(Resource, Clone)]
pub struct EventCapture {
    events: Arc<Mutex<VecDeque<CapturedEvent>>>,
    max_events: usize,
}

#[derive(Debug, Clone)]
pub struct CapturedEvent {
    pub event_type: String,
    pub data: String,
    pub timestamp: String,
}

impl EventCapture {
    pub fn new(max_events: usize) -> Self {
        Self {
            events: Arc::new(Mutex::new(VecDeque::new())),
            max_events,
        }
    }

    pub fn push(&self, event_type: String, data: String) {
        let mut events = self.events.lock().unwrap();
        events.push_back(CapturedEvent {
            event_type,
            data,
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
        if events.len() > self.max_events {
            events.pop_front();
        }
    }

    pub fn get_events(&self, event_type: Option<&str>, limit: usize) -> Vec<CapturedEvent> {
        let events = self.events.lock().unwrap();
        let filtered: Vec<CapturedEvent> = if let Some(ty) = event_type {
            events
                .iter()
                .filter(|e| e.event_type.to_lowercase() == ty.to_lowercase())
                .cloned()
                .collect()
        } else {
            events.iter().cloned().collect()
        };
        filtered.into_iter().rev().take(limit).collect()
    }
}

impl Default for EventCapture {
    fn default() -> Self {
        Self::new(1000)
    }
}

// Example of how to capture events:
//
// fn capture_my_events(
//     mut events: EventReader<MyEvent>,
//     event_capture: Res<EventCapture>,
// ) {
//     for event in events.read() {
//         event_capture.push("MyEvent".to_string(), format!("{:?}", event));
//     }
// }
//
// Then add the system to your app:
// app.add_systems(Update, capture_my_events);
