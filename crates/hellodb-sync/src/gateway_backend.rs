//! Gateway sync backend — speaks HTTP to the `hellodb-gateway` Cloudflare Worker.
//!
//! hellodb NEVER talks to Cloudflare APIs directly. It talks HTTP to a Worker
//! the user owns (e.g. `ish.workers.dev`) and the Worker fronts R2 on the
//! backend. This keeps the client simple (one bearer token, one endpoint)
//! and keeps all auth + infrastructure concerns in user-controlled code.
//!
//! # Protocol
//!
//! All endpoints (except `/health`) require `Authorization: Bearer <token>`:
//!
//! - `GET /health` → `{status, version, features}`
//! - `PUT /r2/{key}` with body bytes → 204
//! - `GET /r2/{key}` → bytes (200) or 404
//! - `DELETE /r2/{key}` → 204 or 404
//! - `GET /r2?prefix=<p>&cursor=<c>&limit=<n>` → `{keys, truncated, next_cursor}`
//!
//! # Key encoding
//!
//! hellodb keys are restricted to `[a-zA-Z0-9._/-]` per its content-addressing
//! scheme, so no URL-encoding is required. Slashes inside keys are preserved
//! verbatim — e.g. `namespaces/claude.memory/deltas/1000.delta` maps to
//! `/r2/namespaces/claude.memory/deltas/1000.delta`.

use std::io::Read;
use std::time::Duration;

use serde::Deserialize;

use crate::backend::SyncBackend;
use crate::error::SyncError;

const USER_AGENT: &str = "hellodb-sync/0.1 (+https://github.com/ishpr/hellodb)";
const DEFAULT_TIMEOUT_MS: u64 = 30_000;
/// Hard cap on the response body we will buffer into memory.
/// Generous enough for any realistic sealed delta bundle, but bounded so a
/// misbehaving server can't OOM the client.
const MAX_RESPONSE_BYTES: u64 = 256 * 1024 * 1024;

/// Gateway health payload.
#[derive(Debug, Clone, Deserialize)]
pub struct Health {
    pub status: String,
    pub version: String,
    pub features: Vec<String>,
}

/// Response for `GET /r2?prefix=...`.
#[derive(Debug, Clone, Deserialize)]
struct ListResponse {
    keys: Vec<String>,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    next_cursor: Option<String>,
}

/// Sync backend that talks HTTP to a user-owned Worker gateway.
pub struct GatewaySyncBackend {
    base_url: String,
    token: String,
    agent: ureq::Agent,
    timeout_ms: u64,
}

impl GatewaySyncBackend {
    /// Construct a new gateway client.
    ///
    /// `base_url` should NOT include a trailing slash; e.g. `"https://ish.workers.dev"`.
    /// `token` is the bearer token for authenticated requests. Ideally stored in
    /// the OS keychain and passed in by the caller — this type does not own the
    /// key lifecycle.
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        let timeout = Duration::from_millis(DEFAULT_TIMEOUT_MS);
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(timeout)
            .timeout_read(timeout)
            .timeout_write(timeout)
            .user_agent(USER_AGENT)
            .build();

        Self {
            base_url: trim_trailing_slash(base_url.into()),
            token: token.into(),
            agent,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        }
    }

    /// Override the per-request timeout (connect / read / write). Default is 30s.
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        let timeout = Duration::from_millis(timeout_ms);
        self.agent = ureq::AgentBuilder::new()
            .timeout_connect(timeout)
            .timeout_read(timeout)
            .timeout_write(timeout)
            .user_agent(USER_AGENT)
            .build();
        self.timeout_ms = timeout_ms;
        self
    }

    /// Currently-configured request timeout (ms).
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    /// Hit `/health`. Returns the advertised feature list so callers can
    /// check e.g. "does this gateway support `/embed`?".
    pub fn health(&self) -> Result<Health, SyncError> {
        let url = format!("{}/health", self.base_url);
        let resp = self
            .agent
            .get(&url)
            .call()
            .map_err(map_transport_or_status)?;

        let status = resp.status();
        if !(200..300).contains(&status) {
            return Err(status_to_error(status, read_body_string(resp), "/health"));
        }

        let health: Health = resp
            .into_json()
            .map_err(|e| SyncError::Transport(format!("invalid JSON from /health: {e}")))?;
        Ok(health)
    }

    fn blob_url(&self, key: &str) -> String {
        // Caller is expected to have passed validate_key() first.
        format!("{}/r2/{}", self.base_url, key)
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }
}

/// Validate a blob key before embedding it in a URL path. This must stay in
/// sync with the gateway Worker's `extractKey` validator (gateway/src/r2.ts):
/// charset `[A-Za-z0-9/._-]`, max 512 bytes, no leading `/`, no `.` / `..`
/// path segments, no control chars, no empty segments. Defense in depth —
/// hellodb-sync today only generates content-addressed keys that fit this
/// pattern, but the trait is public and future callers might not.
fn validate_key(key: &str) -> Result<(), SyncError> {
    if key.is_empty() || key.len() > 512 {
        return Err(SyncError::Transport(format!(
            "invalid key (empty or > 512 bytes): {} bytes",
            key.len()
        )));
    }
    if key.starts_with('/') {
        return Err(SyncError::Transport("invalid key (leading slash)".into()));
    }
    for seg in key.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return Err(SyncError::Transport(format!(
                "invalid key segment: {seg:?} in {key:?}"
            )));
        }
    }
    for ch in key.chars() {
        let ok = ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-');
        if !ok {
            return Err(SyncError::Transport(format!(
                "invalid key (disallowed char {ch:?}): {key:?}"
            )));
        }
    }
    Ok(())
}

impl SyncBackend for GatewaySyncBackend {
    fn put_blob(&mut self, key: &str, data: &[u8]) -> Result<(), SyncError> {
        validate_key(key)?;
        let url = self.blob_url(key);
        let resp = self
            .agent
            .put(&url)
            .set("Authorization", &self.auth_header())
            .send_bytes(data);

        match resp {
            Ok(r) => {
                let status = r.status();
                if (200..300).contains(&status) {
                    Ok(())
                } else {
                    Err(status_to_error(status, read_body_string(r), key))
                }
            }
            Err(e) => Err(map_transport_or_status_for_key(e, key)),
        }
    }

    fn get_blob(&self, key: &str) -> Result<Option<Vec<u8>>, SyncError> {
        validate_key(key)?;
        let url = self.blob_url(key);
        let resp = self
            .agent
            .get(&url)
            .set("Authorization", &self.auth_header())
            .call();

        match resp {
            Ok(r) => {
                let status = r.status();
                if (200..300).contains(&status) {
                    let mut buf = Vec::new();
                    r.into_reader()
                        .take(MAX_RESPONSE_BYTES)
                        .read_to_end(&mut buf)
                        .map_err(|e| SyncError::Transport(format!("read body: {e}")))?;
                    Ok(Some(buf))
                } else {
                    Err(status_to_error(status, read_body_string(r), key))
                }
            }
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(e) => Err(map_transport_or_status_for_key(e, key)),
        }
    }

    fn list_blobs(&self, prefix: &str) -> Result<Vec<String>, SyncError> {
        let mut out = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let url = format!("{}/r2", self.base_url);
            let mut req = self
                .agent
                .get(&url)
                .set("Authorization", &self.auth_header())
                .query("prefix", prefix);
            if let Some(ref c) = cursor {
                req = req.query("cursor", c);
            }

            let resp = req.call().map_err(map_transport_or_status)?;
            let status = resp.status();
            if !(200..300).contains(&status) {
                return Err(status_to_error(status, read_body_string(resp), prefix));
            }

            let list: ListResponse = resp
                .into_json()
                .map_err(|e| SyncError::Transport(format!("invalid JSON from list: {e}")))?;
            out.extend(list.keys);

            if list.truncated {
                if let Some(next) = list.next_cursor {
                    cursor = Some(next);
                    continue;
                }
            }
            break;
        }

        Ok(out)
    }

    fn delete_blob(&mut self, key: &str) -> Result<(), SyncError> {
        validate_key(key)?;
        let url = self.blob_url(key);
        let resp = self
            .agent
            .delete(&url)
            .set("Authorization", &self.auth_header())
            .call();

        match resp {
            Ok(r) => {
                let status = r.status();
                if (200..300).contains(&status) {
                    Ok(())
                } else {
                    Err(status_to_error(status, read_body_string(r), key))
                }
            }
            // Deleting a missing key is a no-op per the SyncBackend contract.
            Err(ureq::Error::Status(404, _)) => Ok(()),
            Err(e) => Err(map_transport_or_status_for_key(e, key)),
        }
    }
}

// ───── helpers ─────

fn trim_trailing_slash(mut s: String) -> String {
    while s.ends_with('/') {
        s.pop();
    }
    s
}

/// Map a ureq error that happened before we know the status (network /
/// DNS / TLS / timeout) into a `SyncError`. For `ureq::Error::Status`
/// we still need a context string; prefer `map_transport_or_status_for_key`
/// at call sites that know the key.
fn map_transport_or_status(e: ureq::Error) -> SyncError {
    match e {
        ureq::Error::Status(code, resp) => status_to_error(code, read_body_string(resp), ""),
        ureq::Error::Transport(t) => SyncError::Transport(t.to_string()),
    }
}

fn map_transport_or_status_for_key(e: ureq::Error, key: &str) -> SyncError {
    match e {
        ureq::Error::Status(code, resp) => status_to_error(code, read_body_string(resp), key),
        ureq::Error::Transport(t) => SyncError::Transport(t.to_string()),
    }
}

fn status_to_error(status: u16, body: String, context: &str) -> SyncError {
    match status {
        401 | 403 => SyncError::Auth,
        404 => SyncError::NotFound(context.to_string()),
        413 => SyncError::Http {
            status,
            body: if body.is_empty() {
                "payload too large".to_string()
            } else {
                format!("payload too large: {body}")
            },
        },
        _ => SyncError::Http { status, body },
    }
}

/// Best-effort read of a response body as a string for error messages.
/// Truncates to keep error strings bounded.
fn read_body_string(resp: ureq::Response) -> String {
    let mut buf = String::new();
    let _ = resp.into_reader().take(8 * 1024).read_to_string(&mut buf);
    buf
}

// ───── tests ─────
//
// Tests use a tiny in-process HTTP/1.1 server bound to 127.0.0.1 on an
// OS-assigned port. The server dispatches on (method, path) and hands back
// canned responses. No extra deps — just `std::net::TcpListener`.

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    use std::thread;

    /// Handler function producing a canned response for a recorded request.
    type Handler = Box<dyn Fn(&RecordedRequest) -> Canned + Send + Sync>;
    /// A single route entry: (method, path_prefix, handler).
    type Route = (&'static str, &'static str, Handler);

    /// A single canned response keyed on `(method, path_prefix)`.
    struct Canned {
        status: u16,
        reason: &'static str,
        headers: Vec<(&'static str, String)>,
        body: Vec<u8>,
    }

    impl Canned {
        fn new(status: u16, reason: &'static str, body: Vec<u8>) -> Self {
            Self {
                status,
                reason,
                headers: Vec::new(),
                body,
            }
        }
        fn with_header(mut self, k: &'static str, v: impl Into<String>) -> Self {
            self.headers.push((k, v.into()));
            self
        }
    }

    /// Mock HTTP server. Matches on (method, path prefix) — the first registered
    /// matcher wins. The `record` captures the request line + auth header so
    /// tests can assert on them.
    struct MockServer {
        addr: String,
        shutdown: Arc<AtomicBool>,
        record: Arc<Mutex<Vec<RecordedRequest>>>,
        join: Option<thread::JoinHandle<()>>,
    }

    #[derive(Debug, Clone)]
    struct RecordedRequest {
        method: String,
        path: String,
        authorization: Option<String>,
        body: Vec<u8>,
    }

    /// Read the request body after headers. Supports `Content-Length` and
    /// `Transfer-Encoding: chunked`. ureq may use chunked encoding for PUT
    /// bodies; if we only read `Content-Length` bytes, leftover bytes break
    /// the connection and the client sees `Unexpected EOF` (flaky tests).
    fn read_request_body<R: BufRead>(
        reader: &mut R,
        headers: &HashMap<String, String>,
    ) -> std::io::Result<Vec<u8>> {
        let te = headers
            .get("transfer-encoding")
            .map(|s| s.as_str())
            .unwrap_or("");
        if te.to_ascii_lowercase().contains("chunked") {
            let mut body = Vec::new();
            loop {
                let mut size_line = String::new();
                reader.read_line(&mut size_line)?;
                let hex_part = size_line.trim().split(';').next().unwrap_or("").trim();
                let chunk_len = usize::from_str_radix(hex_part, 16).map_err(|_| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, "bad chunk size line")
                })?;
                if chunk_len == 0 {
                    loop {
                        let mut line = String::new();
                        reader.read_line(&mut line)?;
                        if line == "\r\n" || line == "\n" || line.is_empty() {
                            break;
                        }
                    }
                    break;
                }
                let mut chunk = vec![0u8; chunk_len];
                reader.read_exact(&mut chunk)?;
                body.extend_from_slice(&chunk);
                let mut crlf = [0u8; 2];
                reader.read_exact(&mut crlf)?;
            }
            Ok(body)
        } else {
            let content_length = headers
                .get("content-length")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let mut body = vec![0u8; content_length];
            if content_length > 0 {
                reader.read_exact(&mut body)?;
            }
            Ok(body)
        }
    }

    impl MockServer {
        fn start(routes: Vec<Route>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
            listener.set_nonblocking(false).expect("blocking listener");
            let addr = format!("http://{}", listener.local_addr().unwrap());
            let shutdown = Arc::new(AtomicBool::new(false));
            let record = Arc::new(Mutex::new(Vec::<RecordedRequest>::new()));

            let shutdown_c = Arc::clone(&shutdown);
            let record_c = Arc::clone(&record);

            let join = thread::spawn(move || {
                // Short accept timeout so shutdown is prompt.
                listener
                    .set_nonblocking(true)
                    .expect("listener nonblocking");
                while !shutdown_c.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let routes_ref = &routes;
                            let record_c2 = Arc::clone(&record_c);
                            handle_conn(stream, routes_ref, record_c2);
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            });

            MockServer {
                addr,
                shutdown,
                record,
                join: Some(join),
            }
        }

        fn url(&self) -> &str {
            &self.addr
        }

        fn requests(&self) -> Vec<RecordedRequest> {
            self.record.lock().unwrap().clone()
        }
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::SeqCst);
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    fn handle_conn(
        mut stream: std::net::TcpStream,
        routes: &[Route],
        record: Arc<Mutex<Vec<RecordedRequest>>>,
    ) {
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(5))).ok();

        let peer = match stream.try_clone() {
            Ok(p) => p,
            Err(_) => return,
        };
        let mut reader = BufReader::new(peer);

        // Read request line.
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.is_empty() {
            return;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            return;
        }
        let method = parts[0].to_string();
        let path = parts[1].to_string();

        // Read headers.
        let mut headers = HashMap::new();
        loop {
            let mut hline = String::new();
            if reader.read_line(&mut hline).is_err() {
                return;
            }
            if hline == "\r\n" || hline == "\n" || hline.is_empty() {
                break;
            }
            if let Some(idx) = hline.find(':') {
                let (k, v) = hline.split_at(idx);
                headers.insert(k.trim().to_ascii_lowercase(), v[1..].trim().to_string());
            }
        }

        let body = match read_request_body(&mut reader, &headers) {
            Ok(b) => b,
            Err(_) => return,
        };

        let req = RecordedRequest {
            method: method.clone(),
            path: path.clone(),
            authorization: headers.get("authorization").cloned(),
            body,
        };
        record.lock().unwrap().push(req.clone());

        // Match route: same method + path starts with prefix.
        let matched = routes
            .iter()
            .find(|(m, p, _)| m.eq_ignore_ascii_case(&method) && path.starts_with(p));

        let response: Canned = match matched {
            Some((_, _, builder)) => builder(&req),
            None => Canned::new(404, "Not Found", b"no route".to_vec()),
        };

        // Write response.
        let mut out = Vec::new();
        out.extend_from_slice(
            format!("HTTP/1.1 {} {}\r\n", response.status, response.reason).as_bytes(),
        );
        out.extend_from_slice(format!("Content-Length: {}\r\n", response.body.len()).as_bytes());
        out.extend_from_slice(b"Connection: close\r\n");
        for (k, v) in &response.headers {
            out.extend_from_slice(format!("{k}: {v}\r\n").as_bytes());
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&response.body);
        let _ = stream.write_all(&out);
        let _ = stream.flush();
        let _ = stream.shutdown(std::net::Shutdown::Write);
    }

    // ───── the actual tests ─────

    #[test]
    fn put_then_get_roundtrip() {
        let store: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::new(Mutex::new(HashMap::new()));

        let store_put = Arc::clone(&store);
        let put_handler: Handler = Box::new(move |req| {
            let key = req.path.trim_start_matches("/r2/").to_string();
            store_put.lock().unwrap().insert(key, req.body.clone());
            Canned::new(204, "No Content", Vec::new())
        });

        let store_get = Arc::clone(&store);
        let get_handler: Handler = Box::new(move |req| {
            let key = req.path.trim_start_matches("/r2/").to_string();
            match store_get.lock().unwrap().get(&key) {
                Some(v) => Canned::new(200, "OK", v.clone())
                    .with_header("Content-Type", "application/octet-stream"),
                None => Canned::new(404, "Not Found", b"nope".to_vec()),
            }
        });

        let server = MockServer::start(vec![
            ("PUT", "/r2/", put_handler),
            ("GET", "/r2/", get_handler),
        ]);

        let mut b = GatewaySyncBackend::new(server.url(), "test-token");
        let key = "namespaces/claude.memory/deltas/1000.delta";
        let payload = b"\x00\x01\x02sealed-bytes\xffend".to_vec();
        b.put_blob(key, &payload).unwrap();
        let got = b.get_blob(key).unwrap().unwrap();
        assert_eq!(got, payload);

        // Auth header was sent on both calls.
        let reqs = server.requests();
        assert!(reqs
            .iter()
            .all(|r| r.authorization.as_deref() == Some("Bearer test-token")));
    }

    #[test]
    fn get_missing_returns_not_found() {
        let handler: Handler = Box::new(|_| Canned::new(404, "Not Found", b"missing".to_vec()));
        let server = MockServer::start(vec![("GET", "/r2/", handler)]);

        let b = GatewaySyncBackend::new(server.url(), "tok");
        // Per SyncBackend trait docs, get_blob on a missing key returns Ok(None).
        // Our backend honors that for 404s on GET.
        let res = b.get_blob("missing/key");
        assert!(matches!(res, Ok(None)));
    }

    #[test]
    fn get_returns_not_found_error_on_non_r2_404() {
        // Sanity: if the gateway returns 404 on some other path (e.g. list),
        // we surface SyncError::NotFound.
        let handler: Handler = Box::new(|_| Canned::new(404, "Not Found", b"no prefix".to_vec()));
        let server = MockServer::start(vec![("GET", "/r2", handler)]);

        let b = GatewaySyncBackend::new(server.url(), "tok");
        let err = b.list_blobs("nope/").unwrap_err();
        assert!(matches!(err, SyncError::NotFound(_)), "got: {err:?}");
    }

    #[test]
    fn unauthorized_returns_auth_error() {
        let handler: Handler =
            Box::new(|_| Canned::new(401, "Unauthorized", b"bad token".to_vec()));
        let server = MockServer::start(vec![("GET", "/r2/", handler)]);

        let b = GatewaySyncBackend::new(server.url(), "wrong");
        let err = b.get_blob("any/key").unwrap_err();
        assert!(matches!(err, SyncError::Auth), "got: {err:?}");
    }

    #[test]
    fn list_returns_keys() {
        let handler: Handler = Box::new(|req| {
            // Echo a short fixed list for any prefix query.
            assert!(req.path.starts_with("/r2?") || req.path == "/r2");
            let body = br#"{"keys":["ns/deltas/1.delta","ns/deltas/2.delta"],"truncated":false,"next_cursor":null}"#.to_vec();
            Canned::new(200, "OK", body).with_header("Content-Type", "application/json")
        });
        let server = MockServer::start(vec![("GET", "/r2", handler)]);

        let b = GatewaySyncBackend::new(server.url(), "tok");
        let keys = b.list_blobs("ns/deltas/").unwrap();
        assert_eq!(keys, vec!["ns/deltas/1.delta", "ns/deltas/2.delta"]);
    }

    #[test]
    fn list_follows_cursor_pagination() {
        let hits = Arc::new(Mutex::new(0u32));
        let hits_c = Arc::clone(&hits);
        let handler: Handler = Box::new(move |req| {
            let mut n = hits_c.lock().unwrap();
            *n += 1;
            if *n == 1 {
                assert!(req.path.contains("prefix="));
                assert!(!req.path.contains("cursor="));
                let body = br#"{"keys":["a/1"],"truncated":true,"next_cursor":"c1"}"#.to_vec();
                Canned::new(200, "OK", body)
            } else {
                assert!(req.path.contains("cursor=c1"));
                let body = br#"{"keys":["a/2"],"truncated":false,"next_cursor":null}"#.to_vec();
                Canned::new(200, "OK", body)
            }
        });
        let server = MockServer::start(vec![("GET", "/r2", handler)]);

        let b = GatewaySyncBackend::new(server.url(), "tok");
        let keys = b.list_blobs("a/").unwrap();
        assert_eq!(keys, vec!["a/1", "a/2"]);
        assert_eq!(*hits.lock().unwrap(), 2);
    }

    #[test]
    fn delete_works() {
        let store: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::new(Mutex::new(HashMap::from([(
            "k/1".to_string(),
            b"v".to_vec(),
        )])));

        let store_del = Arc::clone(&store);
        let del_handler: Handler = Box::new(move |req| {
            let key = req.path.trim_start_matches("/r2/").to_string();
            if store_del.lock().unwrap().remove(&key).is_some() {
                Canned::new(204, "No Content", Vec::new())
            } else {
                Canned::new(404, "Not Found", b"gone".to_vec())
            }
        });

        let server = MockServer::start(vec![("DELETE", "/r2/", del_handler)]);

        let mut b = GatewaySyncBackend::new(server.url(), "tok");
        b.delete_blob("k/1").unwrap();
        // Second delete on missing key is a no-op per trait contract.
        b.delete_blob("k/1").unwrap();
    }

    #[test]
    fn health_returns_features() {
        let handler: Handler = Box::new(|req| {
            // Health should not require auth, but tolerate if provided.
            assert_eq!(req.method, "GET");
            let body =
                br#"{"status":"ok","version":"0.2.1","features":["r2","embed","streaming"]}"#
                    .to_vec();
            Canned::new(200, "OK", body).with_header("Content-Type", "application/json")
        });
        let server = MockServer::start(vec![("GET", "/health", handler)]);

        let b = GatewaySyncBackend::new(server.url(), "tok");
        let h = b.health().unwrap();
        assert_eq!(h.status, "ok");
        assert_eq!(h.version, "0.2.1");
        assert_eq!(h.features, vec!["r2", "embed", "streaming"]);
    }

    #[test]
    fn large_body_rejected() {
        let handler: Handler = Box::new(|_| {
            Canned::new(
                413,
                "Payload Too Large",
                b"blob exceeds 100 MiB cap".to_vec(),
            )
        });
        let server = MockServer::start(vec![("PUT", "/r2/", handler)]);

        let mut b = GatewaySyncBackend::new(server.url(), "tok");
        // Non-empty body: exercises both Content-Length and chunked paths in
        // `read_request_body` depending on ureq's framing.
        let err = b.put_blob("huge/key", &[0u8; 16]).unwrap_err();
        match err {
            SyncError::Http { status, body } => {
                assert_eq!(status, 413);
                assert!(body.contains("payload too large"), "body was: {body}");
                assert!(
                    body.contains("100 MiB") || body.contains("exceeds"),
                    "server message not forwarded; got: {body}"
                );
            }
            other => panic!("expected SyncError::Http, got {other:?}"),
        }
    }

    #[test]
    fn transport_error_on_bad_host() {
        // Port 1 is almost certainly closed; connection refused -> transport error.
        let b = GatewaySyncBackend::new("http://127.0.0.1:1", "tok").with_timeout(500);
        let err = b.health().unwrap_err();
        assert!(matches!(err, SyncError::Transport(_)), "got: {err:?}");
    }

    #[test]
    fn trims_trailing_slash_on_base_url() {
        let b = GatewaySyncBackend::new("https://example.com/", "t");
        assert_eq!(b.blob_url("x/y"), "https://example.com/r2/x/y");
        let b2 = GatewaySyncBackend::new("https://example.com///", "t");
        assert_eq!(b2.blob_url("x/y"), "https://example.com/r2/x/y");
    }
}
