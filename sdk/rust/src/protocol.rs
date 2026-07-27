use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const COPPER_BUS_PROTOCOL_V1: &str = "copper.bus/1";
pub const COPPER_ACTIONS_ENDPOINT: &str = "copper-actions";
pub const COPPER_CAPABILITIES_ENDPOINT: &str = "copper-capabilities";
pub const COPPER_JOBS_ENDPOINT: &str = "copper-jobs";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    Clock,
    Fs,
    Keyboard,
    Network,
    Notify,
    SecureStore,
    Shell,
    Store,
    Ui,
    WindowsDisplay,
}

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

#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    Action {
        request_id: String,
        action_id: String,
        input: Map<String, Value>,
    },
    JobResult {
        request_id: String,
        job_id: String,
        result: Value,
    },
    JobError {
        request_id: Option<String>,
        job_id: Option<String>,
        code: String,
        message: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("invalid Copper JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("unsupported Copper protocol '{0}'")]
    UnsupportedProtocol(String),
    #[error("message type is not valid from Bones sender '{0}'")]
    InvalidSender(String),
    #[error("message type is not a host event")]
    UnexpectedMessage,
}

pub fn decode_host_event(sender: &str, payload: &[u8]) -> Result<HostEvent, ProtocolError> {
    let envelope = serde_json::from_slice::<CopperEnvelope>(payload)?;
    let protocol = match &envelope {
        CopperEnvelope::ActionRequest { protocol, .. }
        | CopperEnvelope::CapabilityRequest { protocol, .. }
        | CopperEnvelope::JobAccepted { protocol, .. }
        | CopperEnvelope::JobResult { protocol, .. }
        | CopperEnvelope::Error { protocol, .. } => protocol,
    };
    if protocol != COPPER_BUS_PROTOCOL_V1 {
        return Err(ProtocolError::UnsupportedProtocol(protocol.clone()));
    }

    match envelope {
        CopperEnvelope::ActionRequest {
            request_id,
            action_id,
            input,
            ..
        } if sender == COPPER_ACTIONS_ENDPOINT => Ok(HostEvent::Action {
            request_id,
            action_id,
            input,
        }),
        CopperEnvelope::JobResult {
            request_id,
            job_id,
            result,
            ..
        } if sender == COPPER_JOBS_ENDPOINT => Ok(HostEvent::JobResult {
            request_id,
            job_id,
            result,
        }),
        CopperEnvelope::Error {
            request_id,
            job_id,
            code,
            message,
            ..
        } if sender == COPPER_JOBS_ENDPOINT => Ok(HostEvent::JobError {
            request_id,
            job_id,
            code,
            message,
        }),
        CopperEnvelope::ActionRequest { .. }
        | CopperEnvelope::JobResult { .. }
        | CopperEnvelope::Error { .. } => Err(ProtocolError::InvalidSender(sender.to_string())),
        _ => Err(ProtocolError::UnexpectedMessage),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn host_events_require_the_stamped_copper_sender() {
        let payload = serde_json::to_vec(&json!({
            "type": "action-request",
            "protocol": "copper.bus/1",
            "requestId": "request-1",
            "actionId": "run",
            "input": {}
        }))
        .expect("payload");

        assert!(matches!(
            decode_host_event(COPPER_ACTIONS_ENDPOINT, &payload),
            Ok(HostEvent::Action { action_id, .. }) if action_id == "run"
        ));
        assert!(matches!(
            decode_host_event("untrusted", &payload),
            Err(ProtocolError::InvalidSender(_))
        ));
    }
}
