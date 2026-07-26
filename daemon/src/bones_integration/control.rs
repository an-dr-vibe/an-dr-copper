use bones_bus::{Envelope, Handler, Module, ModuleContext};
use std::sync::{Arc, Mutex};

pub const COPPER_CONTROL_ENDPOINT: &str = "copper-control";
const LIFECYCLE_TOPIC: &str = "core/lifecycle";

#[derive(Debug, Default)]
struct ControlState {
    lifecycle_event_count: usize,
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
}

/// Trusted Copper endpoint registered through Bones' public module contract.
pub struct CopperControlModule {
    handle: CopperControlHandle,
}

impl CopperControlModule {
    pub fn new() -> (Self, CopperControlHandle) {
        let handle = CopperControlHandle::default();
        (
            Self {
                handle: handle.clone(),
            },
            handle,
        )
    }
}

impl Handler for CopperControlModule {
    fn handle(&mut self, envelope: &Envelope) {
        if envelope.topic != LIFECYCLE_TOPIC {
            return;
        }
        if let Ok(mut state) = self.handle.state.lock() {
            state.lifecycle_event_count = state.lifecycle_event_count.saturating_add(1);
        }
    }
}

impl Module for CopperControlModule {
    fn name(&self) -> &str {
        COPPER_CONTROL_ENDPOINT
    }

    fn init(&mut self, context: &mut ModuleContext) -> Result<(), String> {
        context.subscribe(LIFECYCLE_TOPIC);
        Ok(())
    }

    fn respond(&mut self, _sender: &str, payload: &[u8]) -> Option<Vec<u8>> {
        if payload != b"health" {
            return None;
        }
        serde_json::to_vec(&serde_json::json!({
            "ok": true,
            "lifecycleEvents": self.handle.lifecycle_event_count(),
        }))
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::{CopperControlModule, COPPER_CONTROL_ENDPOINT, LIFECYCLE_TOPIC};
    use bones_bus::{Bus, Envelope, Module, ModuleContext, ServiceRegistry};

    #[test]
    fn module_uses_stable_endpoint_and_lifecycle_subscription() {
        let (mut module, _handle) = CopperControlModule::new();
        assert_eq!(module.name(), COPPER_CONTROL_ENDPOINT);
        let mut services = ServiceRegistry::new();
        let mut context = ModuleContext::new(&mut services);
        module.init(&mut context).expect("initialize");
        assert_eq!(context.into_subscriptions(), vec![LIFECYCLE_TOPIC]);
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
            topic: LIFECYCLE_TOPIC.to_string(),
            sender: "bones-host".to_string(),
            correlation: None,
            payload: b"loaded:session-counter".to_vec(),
        });
        bus.dispatch();

        assert_eq!(handle.lifecycle_event_count(), 1);
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
