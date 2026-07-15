//! The embedded, loopback-only HTTP server and its versioned JSON API.
//!
//! Implemented directly on `std::net::TcpListener` (no `axum`/`hyper`/
//! web-framework dependency), bound to `127.0.0.1` only — never a wildcard
//! address — with no authentication, no TLS, and no write endpoints. See
//! `docs/DUNGEON-CRAWLER.md#the-local-http-server`.
//!
//! [`route`] is the pure, sans-socket request handler: given a map, an HTTP
//! method, and a path, it returns the response that would be written to the
//! wire. This is the piece exercised directly ("in-process client") by the
//! test suite; [`serve`] is the thin `TcpListener` loop wrapped around it.

use crate::dungeon::links;
use crate::dungeon::map::DungeonMap;
use crate::dungeon::{commands, render};
use serde_json::json;
use std::io::{self, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// `GET /` — the rendered dungeon map.
pub const ROUTE_INDEX: &str = "/";
/// `GET /api/v1/rooms` — JSON list of rooms (id, name, region, position).
pub const ROUTE_ROOMS: &str = "/api/v1/rooms";
/// `GET /api/v1/rooms/<id>` prefix — JSON detail for one room.
pub const ROUTE_ROOM_PREFIX: &str = "/api/v1/rooms/";
/// `GET /api/v1/resources/<id>` prefix — JSON detail for one resource.
pub const ROUTE_RESOURCE_PREFIX: &str = "/api/v1/resources/";

/// A response the caller (either the test suite directly, or [`serve`]'s
/// socket loop) turns into bytes on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

/// Route a single request against `map`, entirely in-process (no socket
/// involved). Every route is read-only; there is no route that can mutate
/// `map` or reach back out to Azure.
///
/// * `GET /` -> the rendered HTML map (200).
/// * `GET /api/v1/rooms` -> JSON array of room summaries (200).
/// * `GET /api/v1/rooms/<id>` -> JSON room detail, or 404 if `<id>` is
///   unknown.
/// * `GET /api/v1/resources/<id>` -> JSON resource detail (icon, portal
///   link, suggested commands), or 404 if `<id>` is unknown. Resource ids
///   may contain `/`, so this match is a prefix match against the rest of
///   the path, not a path-segment match.
/// * Anything else (unknown path, or non-`GET` method) -> 404/405.
pub fn route(map: &DungeonMap, method: &str, path: &str) -> RouteResponse {
    if method != "GET" {
        return RouteResponse {
            status: 405,
            content_type: "text/plain; charset=utf-8",
            body: "405 Method Not Allowed: this server is read-only".to_string(),
        };
    }

    if path == ROUTE_INDEX {
        return RouteResponse {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: render::render_html(map),
        };
    }

    if path == ROUTE_ROOMS {
        let summaries: Vec<_> = map
            .rooms
            .iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "name": r.name,
                    "region": r.region,
                    "x": r.x,
                    "y": r.y,
                    "resource_count": r.resources.len(),
                })
            })
            .collect();
        return RouteResponse {
            status: 200,
            content_type: "application/json; charset=utf-8",
            body: serde_json::to_string(&summaries).unwrap_or_else(|_| "[]".to_string()),
        };
    }

    if let Some(room_id) = path.strip_prefix(ROUTE_ROOM_PREFIX) {
        return match map.room(room_id) {
            Some(room) => {
                let body = json!({
                    "id": room.id,
                    "name": room.name,
                    "region": room.region,
                    "x": room.x,
                    "y": room.y,
                    "resources": room.resources.iter().map(|r| json!({
                        "id": r.id,
                        "name": r.name,
                        "kind": r.kind,
                        "region": r.region,
                        "icon": r.icon,
                    })).collect::<Vec<_>>(),
                });
                RouteResponse {
                    status: 200,
                    content_type: "application/json; charset=utf-8",
                    body: serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string()),
                }
            }
            None => not_found(),
        };
    }

    if let Some(resource_id) = path.strip_prefix(ROUTE_RESOURCE_PREFIX) {
        return match map.resource(resource_id) {
            Some(res) => {
                let body = json!({
                    "id": res.id,
                    "name": res.name,
                    "kind": res.kind,
                    "region": res.region,
                    "icon": res.icon,
                    "portal_url": links::portal_url(&res.id),
                    "suggested_commands": commands::suggested_commands(&res.kind, &res.id),
                });
                RouteResponse {
                    status: 200,
                    content_type: "application/json; charset=utf-8",
                    body: serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string()),
                }
            }
            None => not_found(),
        };
    }

    not_found()
}

/// Read a single `\n`-terminated line from `reader`, refusing to grow the
/// buffer past [`MAX_LINE_LEN`]. An oversized line (or one with no
/// terminator before the cap) is treated as a hard I/O error, ending the
/// connection instead of letting an unauthenticated peer force unbounded
/// buffer growth.
fn read_bounded_line(reader: &mut BufReader<TcpStream>) -> io::Result<String> {
    let mut line = String::new();
    loop {
        let mut byte = [0u8; 1];
        match reader.read(&mut byte)? {
            0 => break,
            _ => {
                line.push(byte[0] as char);
                if byte[0] == b'\n' {
                    break;
                }
                if line.len() > MAX_LINE_LEN {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "request line exceeded maximum allowed length",
                    ));
                }
            }
        }
    }
    Ok(line)
}

fn not_found() -> RouteResponse {
    RouteResponse {
        status: 404,
        content_type: "text/plain; charset=utf-8",
        body: "404 Not Found".to_string(),
    }
}

/// A running server: owns the accept loop on a background thread until
/// [`ServerHandle::shutdown`] is called or the handle is dropped.
pub struct ServerHandle {
    addr: SocketAddr,
    shutdown_flag: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl ServerHandle {
    /// The bound loopback address, including whichever port the OS assigned
    /// when `bind_addr`'s port was `0`.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Stop accepting new connections and join the server thread.
    ///
    /// Identical to letting the handle simply go out of scope: the actual
    /// shutdown (flag + wake + join) lives once, in [`Drop`], so this and an
    /// implicit drop can never drift out of sync.
    pub fn shutdown(self) {}
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.shutdown_flag.store(true, Ordering::SeqCst);
        if let Some(handle) = self.join.take() {
            // Nudge the accept loop past its blocking `accept()` call with a
            // harmless local connection so it notices the shutdown flag
            // promptly instead of waiting for the next real client.
            let _ = TcpStream::connect(self.addr);
            let _ = handle.join();
        }
    }
}

/// Start serving `map` on `bind_addr` (must be a `127.0.0.1:<port>` address;
/// `<port>` of `0` lets the OS assign a free ephemeral port). Returns once
/// the listener is bound and accepting, handing back a [`ServerHandle`] with
/// the resolved address.
pub fn serve(map: DungeonMap, bind_addr: &str) -> io::Result<ServerHandle> {
    let listener = TcpListener::bind(bind_addr)?;
    let addr = listener.local_addr()?;
    if !addr.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("refusing to bind to non-loopback address {addr}: Dungeon Crawler Mode's embedded server must only ever listen on 127.0.0.1"),
        ));
    }

    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let thread_flag = Arc::clone(&shutdown_flag);
    let map = Arc::new(map);

    let join = std::thread::spawn(move || {
        for stream in listener.incoming() {
            if thread_flag.load(Ordering::SeqCst) {
                break;
            }
            if let Ok(stream) = stream {
                let _ = handle_connection(stream, &map);
            }
        }
    });

    Ok(ServerHandle {
        addr,
        shutdown_flag,
        join: Some(join),
    })
}

/// Hard cap on a single request-line or header-line length. This server
/// never needs long lines (the routes it serves take short paths and no
/// meaningful headers); the cap exists purely to bound how much an
/// unauthenticated peer can make a connection allocate before we give up.
const MAX_LINE_LEN: usize = 8 * 1024;
/// Hard cap on how much of a request body we will ever buffer.
/// This server has no route that reads the body, so any `Content-Length`
/// is drained (to keep the connection well-formed) but never trusted as an
/// allocation size directly — a malicious/huge value must not make us try
/// to allocate multiple gigabytes up front.
const MAX_BODY_LEN: u64 = 1024 * 1024;

/// Read one HTTP/1.x request line + headers off `stream`, route it, and
/// write back the response. Best-effort: any I/O or parse error just ends
/// the connection rather than panicking the server thread.
fn handle_connection(mut stream: TcpStream, map: &DungeonMap) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);

    let request_line = read_bounded_line(&mut reader)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("/").to_string();

    // Drain the rest of the headers (up to the blank line) without acting on
    // them; this server has no auth and no request body handling.
    let mut content_length: u64 = 0;
    loop {
        let line = read_bounded_line(&mut reader)?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(v) = trimmed
            .to_lowercase()
            .strip_prefix("content-length:")
            .map(|s| s.trim().to_string())
        {
            content_length = v.parse().unwrap_or(0);
        }
    }
    if content_length > 0 {
        // Never pre-allocate a buffer sized directly off an attacker-supplied
        // header: cap what we're willing to read/discard, and stream it
        // through a small fixed buffer rather than one big `Vec`.
        let mut budget = content_length.min(MAX_BODY_LEN);
        let mut chunk = [0u8; 4096];
        while budget > 0 {
            let want = budget.min(chunk.len() as u64) as usize;
            match reader.read(&mut chunk[..want]) {
                Ok(0) => break,
                Ok(n) => budget -= n as u64,
                Err(_) => break,
            }
        }
    }

    let resp = route(map, &method, &path);
    let status_text = match resp.status {
        200 => "200 OK",
        404 => "404 Not Found",
        405 => "405 Method Not Allowed",
        other => return Err(io::Error::other(format!("unexpected route status {other}"))),
    };
    let header = format!(
        "HTTP/1.1 {status_text}\r\nContent-Type: {ct}\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n",
        status_text = status_text,
        ct = resp.content_type,
        len = resp.body.len(),
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(resp.body.as_bytes())?;
    stream.flush()?;
    Ok(())
}
