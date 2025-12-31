//! Container Events
//!
//! This module provides event streaming for container lifecycle events.

use crate::types::{ContainerEvent, ContainerEventType};
use std::sync::Arc;
use tokio::sync::broadcast;

/// Event broadcaster for container events
#[derive(Clone)]
pub struct EventBroadcaster {
    sender: broadcast::Sender<ContainerEvent>,
}

impl EventBroadcaster {
    /// Create a new event broadcaster
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Subscribe to events
    pub fn subscribe(&self) -> EventReceiver {
        EventReceiver {
            receiver: self.sender.subscribe(),
        }
    }

    /// Send an event
    pub fn send(&self, event: ContainerEvent) {
        // Ignore send errors (no receivers)
        let _ = self.sender.send(event);
    }

    /// Create a container event and send it
    pub fn emit(&self, event_type: ContainerEventType, container_id: impl Into<String>) {
        self.send(ContainerEvent::new(event_type, container_id));
    }

    /// Emit a create event
    pub fn emit_create(&self, container_id: impl Into<String>) {
        self.emit(ContainerEventType::Create, container_id);
    }

    /// Emit a start event
    pub fn emit_start(&self, container_id: impl Into<String>) {
        self.emit(ContainerEventType::Start, container_id);
    }

    /// Emit a stop event
    pub fn emit_stop(&self, container_id: impl Into<String>) {
        self.emit(ContainerEventType::Stop, container_id);
    }

    /// Emit a die event with exit code
    pub fn emit_die(&self, container_id: impl Into<String>, exit_code: i32) {
        self.send(
            ContainerEvent::new(ContainerEventType::Die, container_id).with_exit_code(exit_code),
        );
    }

    /// Emit a delete event
    pub fn emit_delete(&self, container_id: impl Into<String>) {
        self.emit(ContainerEventType::Delete, container_id);
    }

    /// Emit an OOM event
    pub fn emit_oom(&self, container_id: impl Into<String>) {
        self.emit(ContainerEventType::Oom, container_id);
    }

    /// Emit a health check event
    pub fn emit_health(&self, container_id: impl Into<String>, healthy: bool) {
        let event_type = if healthy {
            ContainerEventType::HealthOk
        } else {
            ContainerEventType::HealthFail
        };
        self.emit(event_type, container_id);
    }
}

impl Default for EventBroadcaster {
    fn default() -> Self {
        Self::new(256)
    }
}

/// Receiver for container events
pub struct EventReceiver {
    receiver: broadcast::Receiver<ContainerEvent>,
}

impl EventReceiver {
    /// Receive the next event (async)
    pub async fn recv(&mut self) -> Option<ContainerEvent> {
        loop {
            match self.receiver.recv().await {
                Ok(event) => return Some(event),
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Skip lagged events, continue loop
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    /// Try to receive an event without waiting
    pub fn try_recv(&mut self) -> Option<ContainerEvent> {
        match self.receiver.try_recv() {
            Ok(event) => Some(event),
            Err(_) => None,
        }
    }
}

/// Global event broadcaster (thread-safe singleton)
static GLOBAL_EVENTS: std::sync::OnceLock<Arc<EventBroadcaster>> = std::sync::OnceLock::new();

/// Get the global event broadcaster
pub fn global_events() -> Arc<EventBroadcaster> {
    GLOBAL_EVENTS
        .get_or_init(|| Arc::new(EventBroadcaster::default()))
        .clone()
}

/// Subscribe to global events
pub fn subscribe_events() -> EventReceiver {
    global_events().subscribe()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_broadcast() {
        let broadcaster = EventBroadcaster::new(16);
        let mut receiver = broadcaster.subscribe();

        broadcaster.emit_create("test-container");
        broadcaster.emit_start("test-container");
        broadcaster.emit_stop("test-container");

        let event1 = receiver.recv().await.unwrap();
        assert_eq!(event1.event_type, ContainerEventType::Create);
        assert_eq!(event1.container_id, "test-container");

        let event2 = receiver.recv().await.unwrap();
        assert_eq!(event2.event_type, ContainerEventType::Start);

        let event3 = receiver.recv().await.unwrap();
        assert_eq!(event3.event_type, ContainerEventType::Stop);
    }

    #[tokio::test]
    async fn test_event_with_exit_code() {
        let broadcaster = EventBroadcaster::new(16);
        let mut receiver = broadcaster.subscribe();

        broadcaster.emit_die("test-container", 137);

        let event = receiver.recv().await.unwrap();
        assert_eq!(event.event_type, ContainerEventType::Die);
        assert_eq!(event.exit_code, Some(137));
    }
}
