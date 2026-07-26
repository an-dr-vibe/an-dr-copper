use super::{DaemonError, DaemonState, IpcRequest, IpcResponse};
use crate::control_plane::{ControlPlaneAuth, UI_AUTH_HEADER};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tiny_http::{Header, Method, Request as HttpRequest, Response as HttpResponse, StatusCode};

pub(super) fn handle_http_request(
    mut request: HttpRequest,
    state: &mut DaemonState,
    running: &AtomicBool,
) -> Result<(), DaemonError> {
    let response = if let Some(expected) = state.auth_token.as_deref() {
        let authorized = request
            .headers()
            .iter()
            .find(|header| header.field.equiv(UI_AUTH_HEADER))
            .map(|header| header.value.as_str() == expected)
            .unwrap_or(false);
        if !authorized {
            IpcResponse::err("unauthorized request")
        } else {
            handle_http_route(&mut request, state, running)?
        }
    } else {
        handle_http_route(&mut request, state, running)?
    };

    respond_json(request, response)?;
    Ok(())
}

pub fn send_request(bind_addr: &str, request: &IpcRequest) -> Result<IpcResponse, DaemonError> {
    let auth_token = ControlPlaneAuth::load_persisted_token()?;
    let url = request_url(bind_addr, request);
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build();
    let request_builder = match request {
        IpcRequest::Health => agent.get(&url),
        IpcRequest::List => agent.get(&url),
        IpcRequest::Trigger { .. } => agent.post(&url),
        IpcRequest::Reload => agent.post(&url),
        IpcRequest::Verify => agent.post(&url),
        IpcRequest::Shutdown => agent.post(&url),
    };
    let request_builder = if let Some(token) = auth_token.as_deref() {
        request_builder.set(UI_AUTH_HEADER, token)
    } else {
        request_builder
    };

    let response = match request {
        IpcRequest::Trigger { id, action } => {
            request_builder.send_string(&serde_json::to_string(&serde_json::json!({
                "id": id,
                "action": action,
            }))?)
        }
        _ => request_builder.call(),
    };

    match response {
        Ok(response) => parse_http_response(response.status(), response.into_string()?),
        Err(ureq::Error::Status(code, response)) => {
            parse_http_response(code, response.into_string().unwrap_or_default())
        }
        Err(ureq::Error::Transport(err)) => Err(DaemonError::Protocol(err.to_string())),
    }
}

pub(super) fn request_url(bind_addr: &str, request: &IpcRequest) -> String {
    let base = format!("http://{bind_addr}");
    match request {
        IpcRequest::Health => format!("{base}/health"),
        IpcRequest::List => format!("{base}/list"),
        IpcRequest::Trigger { .. } => format!("{base}/trigger"),
        IpcRequest::Reload => format!("{base}/reload"),
        IpcRequest::Verify => format!("{base}/verify"),
        IpcRequest::Shutdown => format!("{base}/shutdown"),
    }
}

pub(super) fn parse_http_response(
    status_code: u16,
    body: String,
) -> Result<IpcResponse, DaemonError> {
    if body.trim().is_empty() {
        return Err(DaemonError::Protocol(format!(
            "daemon returned an empty HTTP response with status {status_code}"
        )));
    }
    let response: IpcResponse = serde_json::from_str(body.trim())?;
    Ok(response)
}

pub(super) fn handle_http_route(
    request: &mut HttpRequest,
    state: &mut DaemonState,
    running: &AtomicBool,
) -> Result<IpcResponse, DaemonError> {
    let ipc_request = match (request.method(), request.url()) {
        (&Method::Get, "/health") => IpcRequest::Health,
        (&Method::Get, "/list") => IpcRequest::List,
        (&Method::Post, "/reload") => IpcRequest::Reload,
        (&Method::Post, "/verify") => IpcRequest::Verify,
        (&Method::Post, "/shutdown") => IpcRequest::Shutdown,
        (&Method::Post, "/trigger") => {
            let mut body = String::new();
            request.as_reader().read_to_string(&mut body)?;

            #[derive(Deserialize)]
            struct TriggerBody {
                id: String,
                #[serde(default)]
                action: Option<String>,
            }

            match serde_json::from_str::<TriggerBody>(&body) {
                Ok(body) => IpcRequest::Trigger {
                    id: body.id,
                    action: body.action,
                },
                Err(err) => return Ok(IpcResponse::err(format!("invalid request: {err}"))),
            }
        }
        _ => return Ok(IpcResponse::err("unsupported request")),
    };

    handle_request(state, ipc_request, running)
}

pub(super) fn handle_request(
    state: &mut DaemonState,
    ipc_request: IpcRequest,
    running: &AtomicBool,
) -> Result<IpcResponse, DaemonError> {
    Ok(match ipc_request {
        IpcRequest::Health => match state.control_service().health_payload() {
            Ok(data) => IpcResponse::ok("daemon alive", Some(data)),
            Err(message) => IpcResponse::err(message),
        },
        IpcRequest::List => IpcResponse::ok(
            "extensions listed",
            Some(state.control_service().list_payload()),
        ),
        IpcRequest::Trigger { id, action } => match state.trigger_payload(&id, action.as_deref()) {
            Ok(data) => IpcResponse::ok("trigger prepared", Some(data)),
            Err(message) => IpcResponse::err(message),
        },
        IpcRequest::Reload => match state.reload() {
            Ok(count) => IpcResponse::ok(
                format!("reloaded {count} extension(s)"),
                Some(serde_json::json!({ "extensionsLoaded": count })),
            ),
            Err(err) => IpcResponse::err(err.to_string()),
        },
        IpcRequest::Verify => match state.control_service().verify_registry() {
            Ok(count) => IpcResponse::ok(
                format!("verified {count} extension(s)"),
                Some(serde_json::json!({ "extensionsVerified": count })),
            ),
            Err(err) => IpcResponse::err(err),
        },
        IpcRequest::Shutdown => {
            running.store(false, Ordering::Relaxed);
            IpcResponse::ok("shutdown signal accepted", None)
        }
    })
}

fn respond_json(request: HttpRequest, response: IpcResponse) -> Result<(), DaemonError> {
    let status = if response.ok { 200 } else { 400 };
    let payload = serde_json::to_string(&response)?;
    let response = HttpResponse::from_string(payload)
        .with_status_code(StatusCode(status))
        .with_header(
            Header::from_bytes("Content-Type", "application/json")
                .map_err(|_| DaemonError::Protocol("invalid response header".to_string()))?,
        );
    request
        .respond(response)
        .map_err(|err| DaemonError::Protocol(err.to_string()))
}
