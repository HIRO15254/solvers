# Local tiny_http patch

Upstream: tiny_http 0.12.0, crates.io archive SHA-256 `389915df6413a2e74fb181895f933386023c71110878cd0825588928e64cdc82` (MIT OR Apache-2.0).

- `src/util/equal_reader.rs`: do not drain abandoned fixed-length bodies (drain limit: **zero bytes**). This avoids allocations based on untrusted Content-Length and prevents abandoned uploads from blocking the daemon's request loop. The corresponding reader test expects unread bytes to remain.
- `src/client.rs`, `src/request.rs`, `src/response.rs`: close body-bearing connections after one request and advertise `Connection: close`, so unread fixed-length or chunked body bytes can never be parsed as a subsequent request. This also closes a body-bearing connection when the body was fully read.
- `src/connection.rs`, `src/lib.rs`: set a 60-second socket read inactivity timeout before TLS wrapping, including handshakes.
- `src/lib.rs`: `#![allow(warnings)]`. As a path dependency the crate no longer gets Cargo's `--cap-lints allow`, so this keeps `RUSTFLAGS=-D warnings` from failing on lints added by newer compilers.

The upstream per-connection task pool is not capped: doing so cleanly requires revising connection acceptance, including TLS handshake scheduling. The daemon separately limits concurrent request handlers, but idle connections can still consume connection threads. The read timeout is an inactivity timeout, not an absolute deadline; a client that continually sends data can retain a connection or handler.

Drop this patch when an upstream version provides equivalent bounded abandoned-body handling, safe connection closure, and socket timeouts, and the daemon is migrated and its regressions pass.
