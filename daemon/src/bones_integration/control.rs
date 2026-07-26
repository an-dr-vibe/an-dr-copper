use bones_bus::{Envelope, Handler, Module, ModuleContext};
use bones_messages::lifecycle::{Event, LifecycleEvent};
use bones_messages::{DecodeMessage, Message};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

pub const COPPER_CONTROL_ENDPOINT: &str = "copper-control";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CopperLifecycleState {
    Loaded,
    Faulted,
    Reloading,
    Reloaded,
    Stopped,
}

impl From<Event> for CopperLifecycleState {
    fn from(value: Event) -> Self {
        match value {
            Event::Loaded => Self::Loaded,
            Event::Faulted => Self::Faulted,
            Event::Reloading => Self::Reloading,
            Event::Reloaded => Self::Reloaded,
            Event::Stopped => Self::Stopped,
        }
    }
}

#[derive(Debug, Default)]
struct ControlState {
    lifecycle_event_count: usize,
    lifecycle_decode_errors: usize,
    extensions: BTreeMap<String, CopperLifecycleState>,
}

/// Read-only handle used by Copper services to inspect Bones lifecycle state.
#[derive(Debug, Clone, Default)]
pub struct CopperControlHandle {
    state: Arc<Mutex<ControlState>>,
}

impl CopperControlHandle {
    pub fn lifecycle_event_count(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.lifecycle_event_count)
            .unwrap_or_default()
    }

    pub fn lifecycle_decode_errors(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.lifecycle_decode_errors)
            .unwrap_or_default()
    }

    pub fn extensions(&self) -> BTreeMap<String, CopperLifecycleState> {
        self.state
            .lock()
            .map(|state| state.extensions.clone())
            .unwrap_or_default()
    }
}

/// Trusted Copper endpoint registered through Bones' public module contract.
pub struct CopperControlModule {
    handle: CopperControlHandle,
}

impl CopperControlModule {
    pub fn new() -> (Self, CopperControlHandle) {
        let handle = CopperControlHandle::default();
        (Self::from_handle(handle.clone()), handle)
    }

    pub fn from_handle(handle: CopperControlHandle) -> Self {
        Self { handle }
    }
}

impl Handler for CopperControlModule {
    fn handle(&mut self, envelope: &Envelope) {
        if envelope.topic != LifecycleEvent::TOPIC {
            return;
        }
        if let Ok(mut state) = self.handle.state.lock() {
            match LifecycleEvent::decode(&envelope.payload) {
                Ok(event) => {
                    state.lifecycle_event_count = state.lifecycle_event_count.saturating_add(1);
                    state
                        .extensions
                        .insert(event.extension.to_string(), event.event.into());
                }
                Err(_) => {
                    state.lifecycle_decode_errors = state.lifecycle_decode_errors.saturating_add(1);
                }
            }
        }
    }
}

impl Module for CopperControlModule {
    fn name(&self) -> &str {
        COPPER_CONTROL_ENDPOINT
    }

    fn init(&mut self, context: &mut ModuleContext) -> Result<(), String> {
        context.subscribe(LifecycleEvent::TOPIC);
        Ok(())
    }

    fn respond(&mut self, _sender: &str, payload: &[u8]) -> Option<Vec<u8>> {
        if payload != b"health" {
            return None;
        }
        serde_json::to_vec(&serde_json::json!({
            "ok": true,
            "lifecycleEvents": self.handle.lifecycle_event_count(),
            "lifecycleDecodeErrors": self.handle.lifecycle_decode_errors(),
            "extensions": self.handle.extensions(),
        }))
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{CopperControlModule, CopperLifecycleState, COPPER_CONTROL_ENDPOINT};
    use bones_bus::{Bus, Envelope, Module, ModuleContext, ServiceRegistry};
    use bones_messages::lifecycle::{Event, LifecycleEvent};
    use bones_messages::{EncodeMessage, Message};

    #[test]
    fn module_uses_stable_endpoint_and_lifecycle_subscription() {
        let (mut module, _handle) = CopperControlModule::new();
        assert_eq!(module.name(), COPPER_CONTROL_ENDPOINT);
        let mut services = ServiceRegistry::new();
        let mut context = ModuleContext::new(&mut services);
        module.init(&mut context).expect("initialize");
        assert_eq!(context.into_subscriptions(), vec![LifecycleEvent::TOPIC]);
    }

    #[test]
    fn module_observes_lifecycle_without_bones_internals() {
        let bus = Bus::new();
        let (mut module, handle) = CopperControlModule::new();
        let mut services = ServiceRegistry::new();
        let mut context = ModuleContext::new(&mut services);
        module.init(&mut context).expect("initialize");
        let subscriptions = context.into_subscriptions();
        let endpoint = bus.register(module.name().to_string(), module);
        for topic in subscriptions {
            endpoint.subscribe(topic);
        }

        bus.publish(Envelope {
            topic: LifecycleEvent::TOPIC.to_string(),
            sender: "bones-host".to_string(),
            correlation: None,
            payload: LifecycleEvent {
                event: Event::Loaded,
                extension: "session-counter",
            }
            .encode(),
        });
        bus.dispatch();

        assert_eq!(handle.lifecycle_event_count(), 1);
        assert_eq!(
            handle.extensions().get("session-counter"),
            Some(&CopperLifecycleState::Loaded)
        );
    }

    #[test]
    fn module_counts_malformed_lifecycle_payloads_without_changing_state() {
        let bus = Bus::new();
        let (mut module, handle) = CopperControlModule::new();
        let mut services = ServiceRegistry::new();
        let mut context = ModuleContext::new(&mut services);
        module.init(&mut context).expect("initialize");
        let endpoint = bus.register(module.name().to_string(), module);
        for topic in context.into_subscriptions() {
            endpoint.subscribe(topic);
        }

        bus.publish(Envelope {
            topic: LifecycleEvent::TOPIC.to_string(),
            sender: "bones-host".to_string(),
            correlation: None,
            payload: vec![255, b'x'],
        });
        bus.dispatch();

        assert_eq!(handle.lifecycle_event_count(), 0);
        assert_eq!(handle.lifecycle_decode_errors(), 1);
        assert!(handle.extensions().is_empty());
    }

    #[test]
    fn health_response_reports_observed_lifecycle_count() {
        let (mut module, _handle) = CopperControlModule::new();
        let payload = module.respond("test", b"health").expect("health reply");
        let response: serde_json::Value = serde_json::from_slice(&payload).expect("JSON reply");
        assert_eq!(
            response.get("ok").and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            response
                .get("lifecycleEvents")
                .and_then(|value| value.as_u64()),
            Some(0)
        );
        assert!(module.respond("test", b"unknown").is_none());
    }
}
