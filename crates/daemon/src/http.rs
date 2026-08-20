//! HTTP routing and the bearer-token check.
//!
//! Deliberately small: one blocking server, one thread per request, no
//! async runtime. The daemon's work is spawning processes and reading
//! files, and the request rate is a person clicking things.

use std::sync::Arc;

use anyhow::{Context, Result};
use protocol::{CreateRunRequest, ErrorCode, ErrorResponse, ValidateRequest};
use serde::Serialize;
use tiny_http::{Header, Method, Request, Response, Server};

use crate::api::Api;

pub struct Daemon {
    pub api: Api,
    pub token: String,
}

impl Daemon {
    /// Serves until the process is stopped.
    pub fn serve(self, address: &str) -> Result<()> {
        let server =
            Server::http(address).map_err(|error| anyhow::anyhow!("binding {address}: {error}"))?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|address| address.port())
            .unwrap_or_default();
        println!("solversd listening on http://{address} (port {port})");
        println!("runs root: {}", self.api.runs.path().display());
        println!("token: {}", self.token);

        let shared = Arc::new(self);
        for request in server.incoming_requests() {
            let daemon = Arc::clone(&shared);
            // A request that reads a large event page should not hold up a
            // status poll, and a panic in one handler should not take the
            // daemon down with it.
            std::thread::spawn(move || {
                if let Err(error) = daemon.dispatch(request) {
                    eprintln!("request failed: {error:#}");
                }
            });
        }
        Ok(())
    }

    fn dispatch(&self, mut request: Request) -> Result<()> {
        if !self.authorized(&request) {
            return respond_error(
                request,
                ErrorResponse::new(ErrorCode::Unauthorized, "missing or invalid bearer token"),
            );
        }

        let url = request.url().to_string();
        let (path, query) = match url.split_once('?') {
            Some((path, query)) => (path.to_string(), query.to_string()),
            None => (url, String::new()),
        };
        let method = request.method().clone();
        let segments: Vec<&str> = path.trim_matches('/').split('/').collect();

        match (&method, segments.as_slice()) {
            (Method::Get, ["v1"]) => respond_ok(request, &self.api.info()),
            (Method::Post, ["v1", "validate"]) => {
                let body: ValidateRequest = match read_json(&mut request) {
                    Ok(body) => body,
                    Err(error) => return respond_error(request, error),
                };
                respond(request, self.api.validate(&body))
            }
            (Method::Get, ["v1", "runs"]) => {
                self.api.pump();
                respond(request, self.api.list_runs())
            }
            (Method::Post, ["v1", "runs"]) => {
                let body: CreateRunRequest = match read_json(&mut request) {
                    Ok(body) => body,
                    Err(error) => return respond_error(request, error),
                };
                respond(request, self.api.create_run(&body))
            }
            (Method::Get, ["v1", "runs", id]) => {
                self.api.pump();
                respond(request, self.api.run(id))
            }
            (Method::Get, ["v1", "runs", id, "events"]) => {
                let from = query_value(&query, "from")
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
                respond(request, self.api.events(id, from))
            }
            (Method::Post, ["v1", "runs", id, "cancel"]) => {
                let outcome = self.api.cancel(id);
                self.api.pump();
                respond(request, outcome)
            }
            (Method::Post, ["v1", "runs", id, "resume"]) => respond(request, self.api.resume(id)),
            _ => respond_error(
                request,
                ErrorResponse::new(ErrorCode::NotFound, format!("no route for {method} {path}")),
            ),
        }
    }

    /// Constant-time-ish bearer check.
    ///
    /// The token is a 256-bit random value, so a timing side channel is not
    /// the realistic attack here; the check still avoids an early return per
    /// byte because it costs nothing to do.
    fn authorized(&self, request: &Request) -> bool {
        let Some(header) = request
            .headers()
            .iter()
            .find(|header| header.field.equiv("Authorization"))
        else {
            return false;
        };
        let Some(presented) = header.value.as_str().strip_prefix("Bearer ") else {
            return false;
        };
        let expected = self.token.as_bytes();
        let presented = presented.as_bytes();
        if presented.len() != expected.len() {
            return false;
        }
        presented
            .iter()
            .zip(expected)
            .fold(0u8, |difference, (a, b)| difference | (a ^ b))
            == 0
    }
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| value.to_string())
    })
}

fn read_json<T: serde::de::DeserializeOwned>(
    request: &mut Request,
) -> std::result::Result<T, ErrorResponse> {
    let mut body = String::new();
    std::io::Read::read_to_string(request.as_reader(), &mut body)
        .map_err(|error| ErrorResponse::new(ErrorCode::InvalidConfig, error.to_string()))?;
    serde_json::from_str(&body).map_err(|error| {
        ErrorResponse::new(
            ErrorCode::InvalidConfig,
            format!("request body is not the expected JSON: {error}"),
        )
    })
}

fn respond<T: Serialize>(
    request: Request,
    result: std::result::Result<T, ErrorResponse>,
) -> Result<()> {
    match result {
        Ok(value) => respond_ok(request, &value),
        Err(error) => respond_error(request, error),
    }
}

fn respond_ok<T: Serialize>(request: Request, value: &T) -> Result<()> {
    let body = serde_json::to_string(value).context("serializing the response")?;
    send(request, 200, &body)
}

fn respond_error(request: Request, error: ErrorResponse) -> Result<()> {
    let status = error.http_status();
    let body = serde_json::to_string(&error).context("serializing the error")?;
    send(request, status, &body)
}

fn send(request: Request, status: u16, body: &str) -> Result<()> {
    let header = Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..])
        .map_err(|()| anyhow::anyhow!("building the content-type header"))?;
    request
        .respond(
            Response::from_string(body)
                .with_status_code(status)
                .with_header(header),
        )
        .context("writing the response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_read_by_name() {
        assert_eq!(query_value("from=42", "from").as_deref(), Some("42"));
        assert_eq!(query_value("a=1&from=7&b=2", "from").as_deref(), Some("7"));
        assert_eq!(query_value("a=1", "from"), None);
        assert_eq!(query_value("", "from"), None);
    }
}
