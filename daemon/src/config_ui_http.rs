use crate::config_ui::UiConfigError;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HttpMethod {
    Get,
    Post,
}

#[derive(Debug)]
pub(crate) struct HttpRequest {
    pub method: HttpMethod,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct HttpResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn ok_json(value: &Value) -> Result<Self, UiConfigError> {
        Ok(Self {
            status: 200,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec_pretty(value)?,
        })
    }

    pub fn ok_html(html: String) -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: html.into_bytes(),
        }
    }

    pub fn no_content() -> Self {
        Self {
            status: 204,
            content_type: "text/plain; charset=utf-8",
            body: Vec::new(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        let payload = serde_json::json!({ "error": message.into() });
        Self {
            status: 400,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec_pretty(&payload)
                .unwrap_or_else(|_| b"{\"error\":\"bad request\"}".to_vec()),
        }
    }

    pub fn not_found() -> Self {
        let payload = serde_json::json!({ "error": "not found" });
        Self {
            status: 404,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec_pretty(&payload)
                .unwrap_or_else(|_| b"{\"error\":\"not found\"}".to_vec()),
        }
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        let payload = serde_json::json!({ "error": message.into() });
        Self {
            status: 403,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_vec_pretty(&payload)
                .unwrap_or_else(|_| b"{\"error\":\"forbidden\"}".to_vec()),
        }
    }
}

pub(crate) fn parse_request(stream: TcpStream) -> Result<HttpRequest, UiConfigError> {
    let mut reader = BufReader::new(stream);
    let mut first_line = String::new();
    let bytes = reader.read_line(&mut first_line)?;
    if bytes == 0 {
        return Err(UiConfigError::Request("empty request".to_string()));
    }

    let mut parts = first_line.split_whitespace();
    let method = match parts.next() {
        Some("GET") => HttpMethod::Get,
        Some("POST") => HttpMethod::Post,
        Some(other) => {
            return Err(UiConfigError::Request(format!(
                "unsupported method '{other}'"
            )))
        }
        None => return Err(UiConfigError::Request("missing method".to_string())),
    };
    let path = parts
        .next()
        .ok_or_else(|| UiConfigError::Request("missing path".to_string()))?
        .to_string();

    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(
                name.trim().to_ascii_lowercase(),
                value.trim().trim_end_matches('\r').to_string(),
            );
        }
    }

    let body = if let Some(content_length) = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
    {
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body)?;
        }
        body
    } else if headers
        .get("transfer-encoding")
        .map(|value| value.to_ascii_lowercase().contains("chunked"))
        .unwrap_or(false)
    {
        read_chunked_body(&mut reader)?
    } else {
        Vec::new()
    };

    Ok(HttpRequest {
        method,
        path,
        headers,
        body,
    })
}

pub(crate) fn read_chunked_body<R: BufRead>(reader: &mut R) -> Result<Vec<u8>, UiConfigError> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        let read = reader.read_line(&mut size_line)?;
        if read == 0 {
            return Err(UiConfigError::Request(
                "unexpected EOF while reading chunk size".to_string(),
            ));
        }

        let size_hex = size_line
            .trim()
            .split(';')
            .next()
            .ok_or_else(|| UiConfigError::Request("invalid chunk header".to_string()))?;
        let size = usize::from_str_radix(size_hex, 16)
            .map_err(|_| UiConfigError::Request("invalid chunk size".to_string()))?;

        if size == 0 {
            loop {
                let mut trailer = String::new();
                let read = reader.read_line(&mut trailer)?;
                if read == 0 || trailer == "\r\n" {
                    break;
                }
            }
            break;
        }

        let mut chunk = vec![0u8; size];
        reader.read_exact(&mut chunk)?;
        body.extend_from_slice(&chunk);

        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf)?;
        if crlf != [b'\r', b'\n'] {
            return Err(UiConfigError::Request(
                "invalid chunk terminator".to_string(),
            ));
        }
    }
    Ok(body)
}

pub(crate) fn write_response(
    stream: &mut TcpStream,
    response: HttpResponse,
) -> Result<(), UiConfigError> {
    let status_text = match response.status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        status_text,
        response.content_type,
        response.body.len()
    );
    stream.write_all(header.as_bytes())?;
    if !response.body.is_empty() {
        stream.write_all(&response.body)?;
    }
    stream.flush()?;
    Ok(())
}
