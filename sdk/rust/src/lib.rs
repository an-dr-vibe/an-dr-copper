//! Guest-side bindings and helpers for `copper.component/1`.
//!
//! Components get no ambient Copper authority. Native work is requested from
//! `copper-capabilities`; Copper authorizes the Bones-stamped sender against
//! the extension manifest and returns the eventual result from `copper-jobs`.

use serde_json::{Map, Value};

pub mod protocol;

pub mod bindings {
    wit_bindgen::generate!({
        path: "../wit",
        world: "extension",
        pub_export_macro: true,
        default_bindings_module: "copper_component_sdk::bindings",
    });
}

pub use bindings::bones::core::host_api;
pub use bindings::Guest;
pub use protocol::{
    Capability, CopperEnvelope, HostEvent, ProtocolError, COPPER_ACTIONS_ENDPOINT,
    COPPER_BUS_PROTOCOL_V1, COPPER_CAPABILITIES_ENDPOINT, COPPER_JOBS_ENDPOINT,
};

/// A capability request accepted for asynchronous execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedJob {
    pub request_id: String,
    pub job_id: String,
}

/// Failure to encode, deliver, or accept a capability request.
#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("failed to encode or decode Copper JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Copper endpoint is unavailable")]
    UnknownEndpoint,
    #[error("Copper rejected a synchronous send cycle")]
    Cycle,
    #[error("Copper rejected request {request_id}: {code}: {message}")]
    Rejected {
        request_id: String,
        code: String,
        message: String,
    },
    #[error("Copper returned an unexpected response")]
    UnexpectedResponse,
}

/// Sends one permissioned native request.
///
/// `request_id` must be unique for this extension across component reloads.
/// For action work, use the host-issued `HostEvent::Action::request_id`. Derive
/// a distinct token from it when one action sends multiple capability requests.
///
/// Acceptance is synchronous and bounded. The operation result is delivered
/// later to `Guest::on_message` from `copper-jobs`.
pub fn request(
    request_id: impl Into<String>,
    capability: Capability,
    operation: impl Into<String>,
    args: Map<String, Value>,
) -> Result<AcceptedJob, RequestError> {
    request_with(
        request_id,
        capability,
        operation,
        args,
        |endpoint, payload| {
            host_api::send(endpoint, payload).map_err(|error| match error {
                host_api::SendError::UnknownEndpoint => RequestError::UnknownEndpoint,
                host_api::SendError::Cycle => RequestError::Cycle,
            })
        },
    )
}

/// Transport-injected form used by tests and advanced guest adapters.
pub fn request_with<F>(
    request_id: impl Into<String>,
    capability: Capability,
    operation: impl Into<String>,
    args: Map<String, Value>,
    send: F,
) -> Result<AcceptedJob, RequestError>
where
    F: FnOnce(&str, &[u8]) -> Result<Vec<u8>, RequestError>,
{
    let request_id = request_id.into();
    let envelope = CopperEnvelope::CapabilityRequest {
        protocol: COPPER_BUS_PROTOCOL_V1.to_string(),
        request_id: request_id.clone(),
        capability,
        operation: operation.into(),
        args,
    };
    let payload = serde_json::to_vec(&envelope)?;
    let reply = send(COPPER_CAPABILITIES_ENDPOINT, &payload)?;
    match serde_json::from_slice::<CopperEnvelope>(&reply)? {
        CopperEnvelope::JobAccepted {
            protocol,
            request_id: returned_request_id,
            job_id,
        } if protocol == COPPER_BUS_PROTOCOL_V1 && returned_request_id == request_id => {
            Ok(AcceptedJob { request_id, job_id })
        }
        CopperEnvelope::Error {
            protocol,
            request_id: Some(returned_request_id),
            code,
            message,
            ..
        } if protocol == COPPER_BUS_PROTOCOL_V1 && returned_request_id == request_id => {
            Err(RequestError::Rejected {
                request_id,
                code,
                message,
            })
        }
        _ => Err(RequestError::UnexpectedResponse),
    }
}

/// Decodes and authenticates an action or asynchronous result delivered by
/// Copper. Identity is taken from the Bones sender, never from JSON fields.
pub fn decode_host_event(sender: &str, payload: &[u8]) -> Result<HostEvent, ProtocolError> {
    protocol::decode_host_event(sender, payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_uses_versioned_contract_and_correlates_acceptance() {
        let accepted = request_with(
            "action-42",
            Capability::Store,
            "get",
            Map::from_iter([("key".to_string(), json!("count"))]),
            |endpoint, payload| {
                assert_eq!(endpoint, COPPER_CAPABILITIES_ENDPOINT);
                let request: CopperEnvelope =
                    serde_json::from_slice(payload).expect("request envelope");
                let request_id = match request {
                    CopperEnvelope::CapabilityRequest {
                        protocol,
                        request_id,
                        capability,
                        operation,
                        ..
                    } => {
                        assert_eq!(protocol, COPPER_BUS_PROTOCOL_V1);
                        assert_eq!(capability, Capability::Store);
                        assert_eq!(operation, "get");
                        request_id
                    }
                    other => panic!("unexpected request: {other:?}"),
                };
                Ok(serde_json::to_vec(&CopperEnvelope::JobAccepted {
                    protocol: COPPER_BUS_PROTOCOL_V1.to_string(),
                    request_id,
                    job_id: "job-7".to_string(),
                })
                .expect("accepted response"))
            },
        )
        .expect("request accepted");

        assert_eq!(accepted.job_id, "job-7");
        assert_eq!(accepted.request_id, "action-42");
    }
}
