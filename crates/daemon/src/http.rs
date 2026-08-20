//! HTTP routing and the bearer-token check.
//!
//! Deliberately small: one blocking server, one thread per request, no
//! async runtime. The daemon's work is spawning processes and reading
//! files, and the request rate is a person clicking things.

use std::sync::Arc;

use anyhow::{Context, Result};
use protocol::{CreateRunRequest, ErrorCode, ErrorResponse, SolutionView, ValidateRequest};
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
        // The banner comes first, and always. Anything a client waits for
        // -- a port, a readiness signal -- must not be preceded by output
        // whose presence depends on what the runs root happened to hold.
        println!("solversd listening on http://{address} (port {port})");
        println!("runs root: {}", self.api.runs.path().display());
        println!("token: {}", self.token);

        let recovered = self.api.recover();
        if !recovered.requeued.is_empty() {
            println!("resubmitted {} queued run(s)", recovered.requeued.len());
        }
        if !recovered.interrupted.is_empty() {
            println!(
                "{} run(s) were interrupted by a previous daemon and can be resumed: {}",
                recovered.interrupted.len(),
                recovered.interrupted.join(", ")
            );
        }

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
            (Method::Get, ["v1", "runs", id, "artifacts"]) => {
                respond(request, self.api.artifacts(id))
            }
            (Method::Get, ["v1", "runs", id, "artifacts", name]) => {
                match self.api.artifact(id, name) {
                    Ok((bytes, content_type)) => send_bytes(request, 200, &bytes, content_type),
                    Err(error) => respond_error(request, error),
                }
            }
            (Method::Get, ["v1", "runs", id, "solution", view]) => {
                let Some(view) = SolutionView::parse(view) else {
                    return respond_error(
                        request,
                        ErrorResponse::new(
                            ErrorCode::NotFound,
                            format!("{view:?} is not a solution view"),
                        ),
                    );
                };
                let csv = query_value(&query, "format").as_deref() == Some("csv");
                match self.api.solution_view(id, view, csv) {
                    Ok((bytes, content_type)) => send_bytes(request, 200, &bytes, content_type),
                    Err(error) => respond_error(request, error),
                }
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
    send_bytes(request, status, body.as_bytes(), "application/json")
}

/// Artifacts are served as bytes with their own content type: a `.mwsol` is
/// binary and a `.jsonl` is a stream of lines, neither of which a client
/// should be told is a JSON document.
fn send_bytes(request: Request, status: u16, body: &[u8], content_type: &str) -> Result<()> {
    let header = Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
        .map_err(|()| anyhow::anyhow!("building the content-type header"))?;
    request
        .respond(
            Response::from_data(body)
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
