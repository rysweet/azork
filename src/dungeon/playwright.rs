//! The optional, best-effort Playwright-driven renderer.
//!
//! See `docs/DUNGEON-CRAWLER.md#the-optional-playwright-renderer`. This
//! module is intentionally isolated and NEVER required for build, tests, or
//! CI:
//!
//! * It is not compiled against any Playwright/Node.js Cargo dependency —
//!   there isn't one. Driving a real headless browser is an out-of-process,
//!   opt-in concern (a separate Node/Playwright setup, documented here),
//!   not something this crate links against.
//! * [`try_render`] always degrades gracefully: today, that means it always
//!   returns `None` (no external driver is wired up in this build), and any
//!   future wiring must preserve "unavailable/unreachable/failed -> `None`,
//!   never a hard error" so `--playwright` can never turn a working native
//!   render into a failing command.
//! * No Azure resource IDs, secrets, or connection strings are ever sent
//!   anywhere by this module; only the same non-secret shape/label data
//!   already present in the map graph (room names/regions, resource
//!   type/name labels) would ever be forwarded to an external renderer.
//!
//! Real one-time setup (once a browser-driven pass is wired up) would be:
//! `npm install -g playwright && npx playwright install --with-deps chromium`,
//! run from an operator's own machine — never as part of `cargo build`.

use crate::dungeon::map::DungeonMap;

/// Best-effort attempt at a richer, hand-drawn render of `map` via a
/// headless browser. Returns `Some(html)` only on a fully successful
/// external render; returns `None` on any failure, unavailability, or (as
/// today, with no external driver wired in) unconditionally — callers must
/// always fall back to [`crate::dungeon::render::render_html`] when this
/// returns `None`.
pub fn try_render(_map: &DungeonMap) -> Option<String> {
    None
}
