//! TDD (red phase) contract tests for `azork::dungeon::icon_assets`.
//!
//! This module does not exist yet — these tests define the required public
//! API and behavior before implementation:
//!
//! * `pub fn svg_for(icon_key: &str) -> &'static str` — returns an inline
//!   SVG document (as a `&'static str`, embedded at compile time via
//!   `include_str!`, never read from disk at runtime) for a known icon key,
//!   or the bundled fallback ("mystery-chest") SVG for any unrecognized key.
//! * Every bundled icon SVG must be safe to inline directly into the
//!   rendered dungeon map's HTML/SVG document: no `<script>`, no
//!   `onload`/`onclick`/... event-handler attributes, no `javascript:` URIs,
//!   and no external `href`/`xlink:href` references (the whole point is an
//!   offline, self-contained document — nothing may hotlink out).
//!
//! These tests will fail to compile until `src/dungeon/icon_assets.rs` is
//! created and registered as `pub mod icon_assets;` in `src/dungeon/mod.rs`.

use azork::dungeon::icon_assets;
use azork::dungeon::icons;

/// The full set of icon keys defined in `src/dungeon/type_table.rs` today.
/// Kept as a plain list here (rather than depending on the private
/// `type_table` module) so this test file only depends on the public
/// `icon_assets`/`icons` surface.
const KNOWN_ICON_KEYS: &[&str] = &[
    "storage-account",
    "virtual-machine",
    "app-service",
    "key-vault",
    "aks",
    "sql-server",
    "cosmos-db",
    "virtual-network",
    "public-ip",
    "network-security-group",
    "load-balancer",
    "network-interface",
    "resource-group",
];

#[test]
fn svg_for_resolves_every_known_icon_key_to_an_svg_document() {
    for key in KNOWN_ICON_KEYS {
        let svg = icon_assets::svg_for(key);
        assert!(
            svg.contains("<svg"),
            "icon `{key}` must resolve to a document containing an <svg> root element"
        );
        assert!(
            svg.contains("</svg>"),
            "icon `{key}` SVG must be well-formed (closing </svg> tag)"
        );
    }
}

#[test]
fn svg_for_unknown_key_falls_back_to_mystery_chest() {
    let fallback = icon_assets::svg_for(icons::DEFAULT_ICON);
    let unknown = icon_assets::svg_for("totally-unrecognized-icon-key");
    assert_eq!(
        unknown, fallback,
        "an unmapped icon key must fall back to the mystery-chest icon rather than panicking or returning empty content"
    );
    assert!(fallback.contains("<svg"));
}

#[test]
fn svg_for_default_icon_key_itself_resolves() {
    let svg = icon_assets::svg_for(icons::DEFAULT_ICON);
    assert!(svg.contains("<svg"));
}

#[test]
fn all_bundled_icons_contain_no_script_tags() {
    for key in KNOWN_ICON_KEYS.iter().chain([&icons::DEFAULT_ICON]) {
        let svg = icon_assets::svg_for(key).to_lowercase();
        assert!(
            !svg.contains("<script"),
            "icon `{key}` must not embed a <script> tag"
        );
    }
}

#[test]
fn all_bundled_icons_contain_no_inline_event_handlers() {
    for key in KNOWN_ICON_KEYS.iter().chain([&icons::DEFAULT_ICON]) {
        let svg = icon_assets::svg_for(key).to_lowercase();
        for handler in ["onload=", "onclick=", "onerror=", "onmouseover="] {
            assert!(
                !svg.contains(handler),
                "icon `{key}` must not contain the inline event handler attribute `{handler}`"
            );
        }
    }
}

#[test]
fn all_bundled_icons_contain_no_javascript_uris_or_external_references() {
    for key in KNOWN_ICON_KEYS.iter().chain([&icons::DEFAULT_ICON]) {
        let svg = icon_assets::svg_for(key).to_lowercase();
        assert!(
            !svg.contains("javascript:"),
            "icon `{key}` must not contain a javascript: URI"
        );
        assert!(
            !svg.contains("href=\"http://") && !svg.contains("href=\"https://"),
            "icon `{key}` must not reference an external href/xlink:href (offline, self-contained requirement)"
        );
    }
}

#[test]
fn svg_for_is_a_pure_deterministic_function() {
    for key in KNOWN_ICON_KEYS {
        let a = icon_assets::svg_for(key);
        let b = icon_assets::svg_for(key);
        assert_eq!(
            a, b,
            "icon `{key}` lookup must be stable across repeated calls"
        );
    }
}
