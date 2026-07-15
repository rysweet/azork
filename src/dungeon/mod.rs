//! Dungeon Crawler Mode — map an entire Azure subscription into an
//! explorable, hand-drawn-style dungeon and serve it over a local HTTP
//! server.
//!
//! See `docs/DUNGEON-CRAWLER.md` for the full design. This module is the
//! single home for all Dungeon Crawler Mode code; every submodule is a
//! self-contained "brick" with a narrow public interface:
//!
//! * [`map`] — the serializable map graph (rooms/resources/edges) and the
//!   read-only, budgeted enumeration that builds it from an [`crate::az_runner::AzRunner`].
//! * [`concurrency`] — the dependency-free adaptive (AIMD) concurrency
//!   limiter and throttle detector shared by [`map`]'s parallel enumeration
//!   and [`crate::backend::az`]'s retry loop.
//! * [`icons`] — the single Azure resource type -> icon lookup table.
//! * [`icon_assets`] — the compile-time-embedded original SVG icon bodies
//!   each icon key resolves to (see `assets/azure-icons/LICENSE-NOTICE.md`).
//! * [`decorations`] — purely-decorative, margin-confined border/torch/chest/
//!   dragon dressing, placed with no risk of colliding with rooms or
//!   corridors.
//! * [`commands`] — the (same-table-derived) type -> suggested read-only `az`
//!   command lookup.
//! * [`links`] — Azure portal deep-link construction from a resource ID.
//! * [`render`] — the native, offline, deterministic HTML/SVG renderer.
//! * [`server`] — the embedded, loopback-only HTTP server and its
//!   versioned JSON API.
//! * [`cli`] — argument parsing for the `azork crawl` / `azork dungeon`
//!   subcommand.
//! * [`playwright`] — the optional, always-degrading headless-browser
//!   renderer. Never required for build/tests/CI.

pub mod cli;
pub mod commands;
pub mod concurrency;
pub mod decorations;
pub mod icon_assets;
pub mod icons;
pub mod links;
pub mod map;
pub mod playwright;
pub mod render;
pub mod server;
mod type_table;
mod validate;

pub use map::{CancelToken, DungeonMap, Edge, ResourceNode, Room};
