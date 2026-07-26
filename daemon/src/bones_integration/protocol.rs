use crate::descriptor::Permission;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Version discriminator for Copper-owned JSON messages on the Bones bus.
pub const COPPER_BUS_PROTOCOL_V1: &str = "copper.bus/1";

/// Native capability families exposed through Copper's permission broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    Fs,
    Keyboard,
    Network,
    Notify,
    SecureStore,
    Shell,
    Store,
    Ui,
}

impl Capability {
    /// Returns the manifest permission required before this capability queues.
    pub fn required_permission(self) -> Option<Permission> {
        match self {
            Self::Fs => Some(Permission::Fs),
            Self::Keyboard => Some(Permission::Keyboard),
            Self::Network => Some(Permission::Network),
            Self::Notify => None,
            Self::SecureStore => Some(Permission::SecureStore),
            Self::Shell => Some(Permission::Shell),
            Self::Store => Some(Permission::Store),
            Self::Ui => Some(Permission::Ui),
        }
    }
}

/// Copper-owned JSON messages carried by the Bones byte-payload boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CopperEnvelope {
    ActionRequest {
        protocol: String,
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "actionId")]
        action_id: String,
        #[serde(default)]
        input: Map<String, Value>,
    },
    CapabilityRequest {
        protocol: String,
        #[serde(rename = "requestId")]
        request_id: String,
        capability: Capability,
        operation: String,
        #[serde(default)]
        args: Map<String, Value>,
    },
    JobAccepted {
        protocol: String,
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "jobId")]
        job_id: String,
    },
    JobResult {
        protocol: String,
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "jobId")]
        job_id: String,
        result: Value,
    },
    Error {
        protocol: String,
        #[serde(rename = "requestId", skip_serializing_if = "Option::is_none")]
        request_id: Option<String>,
        #[serde(rename = "jobId", skip_serializing_if = "Option::is_none")]
        job_id: Option<String>,
        code: String,
        message: String,
    },
}

impl CopperEnvelope {
    pub fn error(request_id: Option<String>, code: &str, message: impl Into<String>) -> Self {
        Self::Error {
            protocol: COPPER_BUS_PROTOCOL_V1.to_string(),
            request_id,
            job_id: None,
            code: code.to_string(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Capability, CopperEnvelope, COPPER_BUS_PROTOCOL_V1};
    use serde_json::json;

    #[test]
    fn protocol_envelopes_have_stable_versioned_json_shapes() {
        let action = CopperEnvelope::ActionRequest {
            protocol: COPPER_BUS_PROTOCOL_V1.to_string(),
            request_id: "request-1".to_string(),
            action_id: "run".to_string(),
            input: Default::default(),
        };
        assert_eq!(
            serde_json::to_value(action).expect("action JSON"),
            json!({
                "type": "action-request",
                "protocol": "copper.bus/1",
                "requestId": "request-1",
                "actionId": "run",
                "input": {}
            })
        );

        let capability: CopperEnvelope = serde_json::from_value(json!({
            "type": "capability-request",
            "protocol": "copper.bus/1",
            "requestId": "request-2",
            "capability": "secure-store",
            "operation": "get",
            "args": { "key": "token" }
        }))
        .expect("capability JSON");
        assert!(matches!(
            capability,
            CopperEnvelope::CapabilityRequest {
                capability: Capability::SecureStore,
                ..
            }
        ));
    }
}
