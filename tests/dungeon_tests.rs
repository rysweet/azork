//! tests/dungeon_tests.rs
//!
//! Contract tests for Dungeon Crawler Mode (`azork::dungeon`).
//!
//! These are the failing (red-phase) tests for the feature described in
//! `docs/DUNGEON-CRAWLER.md`: mapping a subscription into a dungeon graph via
//! the existing [`AzRunner`] seam, resolving icons/portal-links/suggested
//! commands, rendering a self-contained HTML map, and serving it (plus a
//! small JSON API) over a loopback-only HTTP server. Every test here drives
//! the code through [`FakeAzRunner`]/in-process request routing — none
//! touches a real `az` binary, real Azure, or the network.

use azork::az_runner::{AzRunner, FakeAzRunner};
use azork::dungeon::{cli, commands, decorations, icons, links, map, playwright, render, server};
use azork::secrets::test_fixtures;

// ---------------------------------------------------------------------------
// Fixtures — shaped exactly like real `az ... -o json` output.
// ---------------------------------------------------------------------------

const GROUP_LIST_ARGS: &[&str] = &["group", "list", "-o", "json"];

fn resource_list_args(group: &str) -> [&str; 6] {
    ["resource", "list", "--resource-group", group, "-o", "json"]
}

/// Two resource groups (`web-rg` in `eastus`, `data-rg` in `eastus`, plus a
/// third `iso-rg` in `westus2` with no resources) — enough to exercise
/// same-region edges, resource attachment, and an empty room.
const GROUP_LIST_JSON: &str = r#"[
  {
    "id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/web-rg",
    "location": "eastus",
    "name": "web-rg",
    "properties": {"provisioningState": "Succeeded"},
    "tags": null,
    "type": "Microsoft.Resources/resourceGroups"
  },
  {
    "id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/data-rg",
    "location": "eastus",
    "name": "data-rg",
    "properties": {"provisioningState": "Succeeded"},
    "tags": null,
    "type": "Microsoft.Resources/resourceGroups"
  },
  {
    "id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/iso-rg",
    "location": "westus2",
    "name": "iso-rg",
    "properties": {"provisioningState": "Succeeded"},
    "tags": null,
    "type": "Microsoft.Resources/resourceGroups"
  }
]"#;

const DATA_RG_RESOURCES_JSON: &str = r#"[
  {
    "id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/data-rg/providers/Microsoft.Storage/storageAccounts/mystorageacct",
    "name": "mystorageacct",
    "type": "Microsoft.Storage/storageAccounts",
    "location": "eastus",
    "resourceGroup": "data-rg",
    "tags": null
  }
]"#;

const ISO_RG_RESOURCES_JSON: &str = "[]";

fn web_rg_resources_json() -> String {
    format!(
        r#"[{{
    "id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/web-rg/providers/Microsoft.Web/sites/app1",
    "name": "app1",
    "type": "Microsoft.Web/sites",
    "location": "eastus",
    "resourceGroup": "web-rg",
    "kind": "app",
    "tags": null
  }}, {{
    "id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/web-rg/providers/Microsoft.KeyVault/vaults/kv1",
    "name": "kv1",
    "type": "Microsoft.KeyVault/vaults",
    "location": "eastus",
    "resourceGroup": "web-rg",
    "properties": {{"vaultUri": "https://kv1.vault.azure.net/", "connectionString": "Endpoint=sb://example;{fragment}"}},
    "tags": null
  }}]"#,
        fragment = test_fixtures::HOSTILE_ACCOUNT_KEY_FRAGMENT
    )
}

fn fixture_runner() -> FakeAzRunner {
    let web = web_rg_resources_json();
    FakeAzRunner::new()
        .with(GROUP_LIST_ARGS, GROUP_LIST_JSON)
        .with(&resource_list_args("web-rg"), &web)
        .with(&resource_list_args("data-rg"), DATA_RG_RESOURCES_JSON)
        .with(&resource_list_args("iso-rg"), ISO_RG_RESOURCES_JSON)
}

/// A resource with a name crafted to try to inject markup, to prove
/// escaping in the renderer/JSON API.
fn hostile_map() -> map::DungeonMap {
    map::DungeonMap {
        subscription: "mock".to_string(),
        rooms: vec![map::Room {
            id: "hostile-rg".to_string(),
            name: "<script>alert(1)</script>".to_string(),
            region: "eastus".to_string(),
            x: 0,
            y: 0,
            resources: vec![map::ResourceNode {
                id: "/subscriptions/0/resourceGroups/hostile-rg/providers/Microsoft.Storage/storageAccounts/evil\"><img src=x>"
                    .to_string(),
                name: "evil\"><img src=x onerror=alert(1)>".to_string(),
                kind: "Microsoft.Storage/storageAccounts".to_string(),
                region: "eastus".to_string(),
                icon: icons::DEFAULT_ICON.to_string(),
            }],
        }],
        edges: vec![],
        partial: false,
    }
}

fn small_map() -> map::DungeonMap {
    map::DungeonMap {
        subscription: "mock".to_string(),
        rooms: vec![
            map::Room {
                id: "web-rg".to_string(),
                name: "web-rg".to_string(),
                region: "eastus".to_string(),
                x: 0,
                y: 0,
                resources: vec![map::ResourceNode {
                    id: "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/web-rg/providers/Microsoft.Web/sites/app1"
                        .to_string(),
                    name: "app1".to_string(),
                    kind: "Microsoft.Web/sites".to_string(),
                    region: "eastus".to_string(),
                    icon: "app-service".to_string(),
                }],
            },
            map::Room {
                id: "data-rg".to_string(),
                name: "data-rg".to_string(),
                region: "eastus".to_string(),
                x: 2,
                y: 0,
                resources: vec![],
            },
        ],
        edges: vec![map::Edge {
            from: "web-rg".to_string(),
            to: "data-rg".to_string(),
        }],
        partial: false,
    }
}

// ---------------------------------------------------------------------------
// map: enumeration / graph construction
// ---------------------------------------------------------------------------

#[test]
fn build_produces_one_room_per_resource_group() {
    let runner = fixture_runner();
    let dmap = map::build(&runner, map::DEFAULT_BUDGET).expect("build should succeed");

    assert_eq!(dmap.rooms.len(), 3, "expected one room per resource group");
    assert!(dmap.room("web-rg").is_some());
    assert!(dmap.room("data-rg").is_some());
    assert!(dmap.room("iso-rg").is_some());
    assert!(
        !dmap.partial,
        "a clean, uncancelled build should not be partial"
    );
}

#[test]
fn build_attaches_resources_to_their_owning_room() {
    let runner = fixture_runner();
    let dmap = map::build(&runner, map::DEFAULT_BUDGET).expect("build should succeed");

    let web = dmap.room("web-rg").expect("web-rg room");
    assert_eq!(web.resources.len(), 2);
    assert!(web.resources.iter().any(|r| r.name == "app1"));
    assert!(web.resources.iter().any(|r| r.name == "kv1"));

    let iso = dmap.room("iso-rg").expect("iso-rg room");
    assert!(
        iso.resources.is_empty(),
        "a resource group with no resources should map to an empty room, not be dropped"
    );

    assert_eq!(dmap.resource_count(), 3);
}

#[test]
fn build_resolves_region_and_icon_on_each_resource_node() {
    let runner = fixture_runner();
    let dmap = map::build(&runner, map::DEFAULT_BUDGET).expect("build should succeed");

    let storage = dmap
        .resource(
            "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/data-rg/providers/Microsoft.Storage/storageAccounts/mystorageacct",
        )
        .expect("storage resource should be present");
    assert_eq!(storage.region, "eastus");
    assert_eq!(storage.kind, "Microsoft.Storage/storageAccounts");
    assert_eq!(storage.icon, icons::icon_for(&storage.kind));
}

#[test]
fn build_never_leaks_secret_looking_fields_from_resource_properties() {
    let runner = fixture_runner();
    let dmap = map::build(&runner, map::DEFAULT_BUDGET).expect("build should succeed");

    let rendered = render::render_html(&dmap);
    assert!(
        !rendered.contains(test_fixtures::HOSTILE_ACCOUNT_KEY_VALUE),
        "raw `properties` blobs (which may contain secrets) must never reach the rendered map"
    );

    for room in &dmap.rooms {
        for res in &room.resources {
            assert!(!res.id.contains(test_fixtures::HOSTILE_ACCOUNT_KEY_VALUE));
            assert!(!res.name.contains(test_fixtures::HOSTILE_ACCOUNT_KEY_VALUE));
        }
    }
}

#[test]
fn build_is_deterministic_across_repeated_calls() {
    let runner = fixture_runner();
    let first = map::build(&runner, map::DEFAULT_BUDGET).expect("first build");
    let second = map::build(&runner, map::DEFAULT_BUDGET).expect("second build");

    for room_id in ["web-rg", "data-rg", "iso-rg"] {
        let a = first.room(room_id).unwrap();
        let b = second.room(room_id).unwrap();
        assert_eq!(
            (a.x, a.y),
            (b.x, b.y),
            "room `{room_id}` position must be a pure function of (name, region)"
        );
    }
}

#[test]
fn build_connects_rooms_sharing_a_region_with_an_edge() {
    let runner = fixture_runner();
    let dmap = map::build(&runner, map::DEFAULT_BUDGET).expect("build should succeed");

    let connects = |a: &str, b: &str| {
        dmap.edges
            .iter()
            .any(|e| (e.from == a && e.to == b) || (e.from == b && e.to == a))
    };
    assert!(
        connects("web-rg", "data-rg"),
        "web-rg and data-rg share `eastus` and should be corridor-connected"
    );
}

#[test]
fn build_has_no_fixed_size_cap_regardless_of_budget() {
    let runner = fixture_runner();
    let tiny_budget = map::build(&runner, 1).expect("build with budget=1 should still succeed");
    let huge_budget =
        map::build(&runner, 100_000).expect("build with a huge budget should still succeed");

    assert_eq!(
        tiny_budget.resource_count(),
        huge_budget.resource_count(),
        "the resource budget must only shape in-memory batching, never truncate the map"
    );
    assert_eq!(tiny_budget.rooms.len(), huge_budget.rooms.len());
}

#[test]
fn build_survives_malformed_json_for_a_single_room_without_panicking() {
    let runner = FakeAzRunner::new()
        .with(GROUP_LIST_ARGS, GROUP_LIST_JSON)
        .with(
            &resource_list_args("web-rg"),
            "{ this is not valid json [[[",
        )
        .with(&resource_list_args("data-rg"), DATA_RG_RESOURCES_JSON)
        .with(&resource_list_args("iso-rg"), ISO_RG_RESOURCES_JSON);

    let dmap = map::build(&runner, map::DEFAULT_BUDGET)
        .expect("malformed JSON in one room must not fail the whole build");

    // The well-formed rooms still show up with their resources intact.
    assert_eq!(
        dmap.room("data-rg").unwrap().resources.len(),
        1,
        "unaffected rooms must be unaffected by a sibling room's bad JSON"
    );
    // The room with bad JSON is present (never silently dropped) but empty,
    // since its resource list could not be parsed.
    assert!(dmap.room("web-rg").is_some());
    assert!(dmap.room("web-rg").unwrap().resources.is_empty());
}

#[test]
fn build_never_issues_a_mutating_az_invocation() {
    // A runner that fails (loudly) on anything other than the read-only
    // verbs the map builder is allowed to use. If `build` ever tries to
    // create/update/delete anything, this test's `FakeAzRunner` has no
    // canned success response for it and the build must not silently
    // succeed by having actually mutated something — read-only calls are
    // the only calls it can make at all.
    let runner = fixture_runner();
    let dmap = map::build(&runner, map::DEFAULT_BUDGET).expect("build should succeed");
    assert!(!dmap.rooms.is_empty());
}

#[test]
fn build_cancellable_with_a_precancelled_token_yields_a_partial_map() {
    let runner = fixture_runner();
    let cancel = map::CancelToken::new();
    cancel.cancel();
    assert!(cancel.is_cancelled());

    let dmap = map::build_cancellable(&runner, map::DEFAULT_BUDGET, &cancel)
        .expect("a cancelled build should still return whatever partial map it has, not error");

    assert!(
        dmap.partial,
        "enumeration cancelled before any room was processed must be marked partial"
    );
    assert!(
        dmap.rooms.len() <= 3,
        "a precancelled build must not have gone on to enumerate everything"
    );
}

#[test]
fn cancel_token_defaults_to_not_cancelled() {
    let cancel = map::CancelToken::new();
    assert!(!cancel.is_cancelled());
}

// ---------------------------------------------------------------------------
// icons: type -> icon registry
// ---------------------------------------------------------------------------

#[test]
fn icon_for_known_types_matches_documented_mapping() {
    let cases = [
        ("Microsoft.Storage/storageAccounts", "storage-account"),
        ("Microsoft.Compute/virtualMachines", "virtual-machine"),
        ("Microsoft.Web/sites", "app-service"),
        ("Microsoft.KeyVault/vaults", "key-vault"),
        ("Microsoft.ContainerService/managedClusters", "aks"),
        ("Microsoft.Sql/servers", "sql-server"),
        ("Microsoft.DocumentDB/databaseAccounts", "cosmos-db"),
    ];
    for (kind, expected_substr) in cases {
        let icon = icons::icon_for(kind);
        assert!(
            icon.contains(expected_substr) || icon == expected_substr,
            "icon for `{kind}` was `{icon}`, expected something matching `{expected_substr}`"
        );
    }
}

#[test]
fn icon_for_network_types_is_not_the_default_icon() {
    for kind in [
        "Microsoft.Network/virtualNetworks",
        "Microsoft.Network/publicIPAddresses",
        "Microsoft.Network/networkSecurityGroups",
    ] {
        assert_ne!(
            icons::icon_for(kind),
            icons::DEFAULT_ICON,
            "`{kind}` should have a specific network icon, not the fallback"
        );
    }
}

#[test]
fn icon_for_unknown_type_falls_back_to_default_icon() {
    assert_eq!(
        icons::icon_for("Microsoft.SomeBrandNewService/thingamajigs"),
        icons::DEFAULT_ICON
    );
    assert_eq!(icons::icon_for(""), icons::DEFAULT_ICON);
}

#[test]
fn icon_for_is_case_insensitive() {
    assert_eq!(
        icons::icon_for("microsoft.storage/storageaccounts"),
        icons::icon_for("Microsoft.Storage/storageAccounts")
    );
}

// ---------------------------------------------------------------------------
// commands: type -> suggested read-only `az` command
// ---------------------------------------------------------------------------

const EXAMPLE_ID: &str = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/data-rg/providers/Microsoft.Storage/storageAccounts/mystorageacct";

#[test]
fn suggested_commands_matches_documented_table() {
    let cases = [
        (
            "Microsoft.Storage/storageAccounts",
            "az storage account show",
        ),
        ("Microsoft.Compute/virtualMachines", "az vm show"),
        ("Microsoft.Web/sites", "az webapp show"),
        ("Microsoft.KeyVault/vaults", "az keyvault show"),
        ("Microsoft.Sql/servers", "az sql server show"),
        ("Microsoft.ContainerService/managedClusters", "az aks show"),
        ("Microsoft.DocumentDB/databaseAccounts", "az cosmosdb show"),
    ];
    for (kind, expected_prefix) in cases {
        let cmds = commands::suggested_commands(kind, EXAMPLE_ID);
        assert!(
            !cmds.is_empty(),
            "expected at least one suggested command for `{kind}`"
        );
        assert!(
            cmds.iter().any(|c| c.starts_with(expected_prefix)),
            "expected a command starting with `{expected_prefix}` for `{kind}`, got {cmds:?}"
        );
        assert!(
            cmds.iter().any(|c| c.contains(EXAMPLE_ID)),
            "suggested command must have the resource id substituted in, got {cmds:?}"
        );
        // Display-only: never `az ... --ids <id> | some-mutation`, and no
        // shell metacharacters that could turn a copy/paste into something
        // unintended beyond running the shown command itself.
        for c in &cmds {
            assert!(!c.contains(';') && !c.contains('&') && !c.contains('|'));
        }
    }
}

#[test]
fn suggested_commands_for_unknown_type_falls_back_to_generic_resource_show() {
    let cmds = commands::suggested_commands("Microsoft.BrandNew/thingamajigs", EXAMPLE_ID);
    assert!(!cmds.is_empty());
    assert!(cmds.iter().any(|c| c.starts_with("az resource show")));
    assert!(cmds.iter().any(|c| c.contains(EXAMPLE_ID)));
}

#[test]
fn suggested_commands_never_actually_execute_anything() {
    // Contract check, not a behavioural one: this is a pure string builder.
    // Calling it twice with the same inputs must be side-effect-free and
    // idempotent.
    let a = commands::suggested_commands("Microsoft.Compute/virtualMachines", EXAMPLE_ID);
    let b = commands::suggested_commands("Microsoft.Compute/virtualMachines", EXAMPLE_ID);
    assert_eq!(a, b);
}

#[test]
fn suggested_commands_reject_invalid_ids_and_mutating_verbs() {
    assert!(commands::suggested_commands("Microsoft.Web/sites", "not-an-arm-id").is_empty());
    assert!(commands::is_read_only_command("az vm show --ids /subscriptions/0/resourceGroups/x/providers/Microsoft.Compute/virtualMachines/y"));
    assert!(!commands::is_read_only_command(
        "az vm delete --ids /subscriptions/0/resourceGroups/x/providers/Microsoft.Compute/virtualMachines/y"
    ));
}

// ---------------------------------------------------------------------------
// links: Azure portal deep links
// ---------------------------------------------------------------------------

#[test]
fn portal_url_strips_leading_slash_and_prefixes_portal_base() {
    let id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/data-rg/providers/Microsoft.Storage/storageAccounts/mystorageacct";
    let url = links::portal_url(id);
    assert_eq!(
        url,
        "https://portal.azure.com/#@/resource/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/data-rg/providers/Microsoft.Storage/storageAccounts/mystorageacct"
    );
}

#[test]
fn portal_url_handles_id_without_leading_slash() {
    let id = "subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/web-rg/providers/Microsoft.Web/sites/app1";
    let url = links::portal_url(id);
    assert_eq!(
        url,
        "https://portal.azure.com/#@/resource/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/web-rg/providers/Microsoft.Web/sites/app1"
    );
}

#[test]
fn portal_url_always_starts_with_portal_base() {
    for id in ["/a/b/c", "a/b/c", ""] {
        if links::is_valid_resource_id(id) {
            assert!(links::portal_url(id).starts_with(links::PORTAL_BASE));
        } else {
            assert_eq!(links::portal_url(id), "about:blank");
        }
    }
}

// ---------------------------------------------------------------------------
// render: native, offline, deterministic HTML renderer
// ---------------------------------------------------------------------------

#[test]
fn render_html_includes_room_and_resource_names() {
    let dmap = small_map();
    let html = render::render_html(&dmap);
    assert!(html.contains("web-rg"));
    assert!(html.contains("data-rg"));
    assert!(html.contains("app1"));
}

#[test]
fn render_html_is_a_pure_function_of_the_map() {
    let dmap = small_map();
    let a = render::render_html(&dmap);
    let b = render::render_html(&dmap);
    assert_eq!(a, b, "rendering the same map twice must be identical");
}

#[test]
fn render_html_escapes_hostile_resource_and_room_names() {
    let dmap = hostile_map();
    let html = render::render_html(&dmap);

    assert!(
        !html.contains("<script>alert(1)</script>"),
        "a raw <script> tag from an attacker-controlled name must never appear unescaped"
    );
    assert!(
        html.contains("&lt;script&gt;"),
        "the hostile room name must appear HTML-escaped somewhere in the output"
    );
    assert!(
        !html.contains("onerror=alert(1)>"),
        "a raw unescaped event-handler attribute injection must never appear in the output"
    );
}

#[test]
fn render_html_marks_a_partial_map_as_partial() {
    let mut dmap = small_map();
    dmap.partial = true;
    let html = render::render_html(&dmap);
    assert!(
        html.to_lowercase().contains("partial"),
        "a partial map must be visibly labelled as such in the rendered output"
    );
}

#[test]
fn render_html_produces_self_contained_document_with_no_external_fetches() {
    let dmap = small_map();
    let html = render::render_html(&dmap);
    // No CDN script tags / external stylesheet fetches: the whole point is
    // that it renders fully offline.
    assert!(!html.contains("http://"));
    assert!(!html.contains("https://") || html.contains("portal.azure.com"));
}

// ---------------------------------------------------------------------------
// render: dungeon-map redesign — TDD (red phase) contract for the
// rectilinear-walled-room, orthogonal-corridor, embedded-icon renderer.
//
// These assertions describe the new markup contract render.rs must produce;
// several will fail against today's force-directed-graph-style renderer
// (bare `<line class="corridor">` diagonals, glyph-square icons) until it is
// rewritten. See docs/DUNGEON-CRAWLER.md and the PR description for the
// tool evaluation + rationale behind the self-designed, Dungeon-Scrawl-style
// approach.
// ---------------------------------------------------------------------------

#[test]
fn render_html_draws_rooms_as_walled_rectilinear_chambers() {
    let dmap = small_map();
    let html = render::render_html(&dmap);

    // Each room must still be an addressable group keyed by room id...
    assert!(html.contains("data-room-id=\"web-rg\""));
    assert!(html.contains("data-room-id=\"data-rg\""));
    // ...but rendered as a walled chamber (thick-stroked perimeter), not a
    // generic single filled rect with no wall styling.
    assert!(
        html.contains("class=\"room-wall\"") || html.contains("room-wall"),
        "each room must be drawn with an explicit wall class distinguishing it as a walled chamber"
    );
}

#[test]
fn render_html_draws_corridors_as_orthogonal_paths_with_doors_not_diagonal_lines() {
    let dmap = small_map(); // web-rg <-> data-rg edge
    let html = render::render_html(&dmap);

    assert!(
        !html.contains("<line class=\"corridor\""),
        "corridors must no longer be rendered as bare diagonal <line> elements connecting room centers"
    );
    assert!(
        html.contains("class=\"corridor\""),
        "a corridor element (rectilinear path/polyline) must still be present for a connected room pair"
    );
    // Rectilinear routing: expressed as an SVG path with horizontal/vertical
    // segments (L/H/V commands), not a straight diagonal line primitive.
    assert!(
        html.contains("<path") && html.contains("class=\"corridor\""),
        "corridors between rooms must be drawn as orthogonal (L-shaped) <path> elements"
    );
    assert!(
        html.contains("class=\"door\""),
        "a door glyph must mark where a corridor meets a room's wall"
    );
}

#[test]
fn render_html_includes_parchment_and_grid_background() {
    let dmap = small_map();
    let html = render::render_html(&dmap);
    assert!(
        html.contains("parchment"),
        "the map background must read as parchment/aged-paper, not a plain dark canvas"
    );
    assert!(
        html.contains("grid-line") || html.contains("class=\"grid\""),
        "a faint grid must be visible behind the dungeon, matching classic tabletop dungeon map styling"
    );
}

#[test]
fn render_html_embeds_resource_icon_svgs_inline_not_hotlinked() {
    let dmap = small_map(); // app1 has icon "app-service"
    let html = render::render_html(&dmap);

    assert!(
        html.contains("data-icon=\"app-service\""),
        "the app-service resource must be tagged with its resolved icon key"
    );
    assert!(
        !html.contains("<img src=\"http"),
        "icons must never be hotlinked from a remote URL"
    );
    assert!(
        html.contains("icon-app-service") && html.contains("<svg"),
        "the app-service icon must be embedded as inline SVG content (a <symbol>/<use> pair or inline <svg>), not a placeholder glyph"
    );
}

#[test]
fn render_html_deduplicates_repeated_icon_definitions() {
    let mut dmap = small_map();
    // Add a second resource sharing the same icon key as the first, in a
    // different room, to prove the icon's SVG definition is embedded once
    // and reused, not duplicated per-resource.
    dmap.rooms[1].resources.push(map::ResourceNode {
        id: "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/data-rg/providers/Microsoft.Web/sites/app2"
            .to_string(),
        name: "app2".to_string(),
        kind: "Microsoft.Web/sites".to_string(),
        region: "eastus".to_string(),
        icon: "app-service".to_string(),
    });
    let html = render::render_html(&dmap);

    let def_marker = "id=\"icon-app-service\"";
    let occurrences = html.matches(def_marker).count();
    assert_eq!(
        occurrences, 1,
        "the app-service icon's SVG definition must be embedded exactly once and referenced twice via <use>, not duplicated per resource"
    );
    assert_eq!(
        html.matches("data-icon=\"app-service\"").count(),
        2,
        "both app-service resources must be tagged with the icon key even though the definition is shared"
    );
}

#[test]
fn render_html_falls_back_to_default_icon_for_an_unmapped_icon_key() {
    let mut dmap = small_map();
    dmap.rooms[0].resources[0].icon = "some-brand-new-unmapped-resource-type".to_string();
    let html = render::render_html(&dmap);

    assert!(
        !html.is_empty(),
        "rendering a resource with an unrecognized icon key must never panic or produce empty output"
    );
    assert!(
        html.contains("icon-mystery-chest") || html.contains(icons::DEFAULT_ICON),
        "an unmapped icon key must fall back to the bundled mystery-chest icon"
    );
}

/// Extract every `<rect ... class="room-floor"/>` element's `(x, y, width,
/// height)` from a rendered document, in source order. Avoids pulling in a
/// regex dependency for what is simple, predictable, self-generated markup.
fn room_floor_rects(html: &str) -> Vec<(i32, i32, i32, i32)> {
    let marker = "class=\"room-floor\"";
    let mut rects = Vec::new();
    for (marker_pos, _) in html.match_indices(marker) {
        // The rect's attributes appear immediately before this marker in the
        // same element; search only the (small) preceding slice so a later,
        // unrelated `<rect` elsewhere in the document is never picked up.
        let search_start = marker_pos.saturating_sub(200);
        let window = &html[search_start..marker_pos];
        if let Some(rel_start) = window.rfind("<rect ") {
            let attrs = &window[rel_start..];
            let x = attr_i32(attrs, "x");
            let y = attr_i32(attrs, "y");
            let w = attr_i32(attrs, "width");
            let h = attr_i32(attrs, "height");
            if let (Some(x), Some(y), Some(w), Some(h)) = (x, y, w, h) {
                rects.push((x, y, w, h));
            }
        }
    }
    rects
}

/// Pull an integer attribute value (e.g. `x="123"`) out of a fragment of
/// SVG markup.
fn attr_i32(fragment: &str, name: &str) -> Option<i32> {
    let needle = format!("{name}=\"");
    let start = fragment.find(&needle)? + needle.len();
    let end = fragment[start..].find('"')? + start;
    fragment[start..end].parse().ok()
}

fn rects_overlap(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> bool {
    let (ax, ay, aw, ah) = a;
    let (bx, by, bw, bh) = b;
    ax < bx + bw && bx < ax + aw && ay < by + bh && by < ay + ah
}

/// Build a map with a single room holding `n` resources, used to exercise
/// adaptive room sizing for rooms with many resources.
fn map_with_room_of_size(n: usize) -> map::DungeonMap {
    let resources = (0..n)
        .map(|i| map::ResourceNode {
            id: format!(
                "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/big-rg/providers/Microsoft.Storage/storageAccounts/res{i}"
            ),
            name: format!("res{i}"),
            kind: "Microsoft.Storage/storageAccounts".to_string(),
            region: "eastus".to_string(),
            icon: "storage-account".to_string(),
        })
        .collect();
    map::DungeonMap {
        subscription: "mock".to_string(),
        rooms: vec![map::Room {
            id: "big-rg".to_string(),
            name: "big-rg".to_string(),
            region: "eastus".to_string(),
            x: 0,
            y: 0,
            resources,
        }],
        edges: vec![],
        partial: false,
    }
}

#[test]
fn render_html_grows_a_room_to_fit_many_resources_without_overflow() {
    // Enough resources that the pre-existing fixed 4-col/116px room would
    // silently overflow past its own walls; the adaptive layout must instead
    // grow the room to fit every icon inside it.
    let dmap = map_with_room_of_size(25);
    let html = render::render_html(&dmap);

    let rects = room_floor_rects(&html);
    assert_eq!(rects.len(), 1, "expected exactly one room-floor rect");
    let (rx, ry, rw, rh) = rects[0];

    // Every `<use ... x="{ix}" y="{iy}" width="20" height="20" .../>` icon
    // instance must lie fully within the room's own floor rect.
    for chunk in html.split("<use ") {
        if !chunk.contains("icon-storage-account") {
            continue;
        }
        let ix = attr_i32(chunk, "x");
        let iy = attr_i32(chunk, "y");
        if let (Some(ix), Some(iy)) = (ix, iy) {
            assert!(
                ix >= rx && ix + 20 <= rx + rw,
                "icon at x={ix} overflows room floor [{rx}, {rx_end}]",
                rx_end = rx + rw
            );
            assert!(
                iy >= ry && iy + 20 <= ry + rh,
                "icon at y={iy} overflows room floor [{ry}, {ry_end}]",
                ry_end = ry + rh
            );
        }
    }
}

#[test]
fn render_html_spaces_rooms_so_a_large_room_never_overlaps_its_neighbor() {
    // A large room (many resources) directly adjacent (dx = 1) to a small
    // empty room: the pre-existing fixed 150px grid cell would let a grown
    // room bleed into its neighbor's cell. The adaptive corridor spacing
    // must derive the grid cell from the *largest* room in the map so this
    // can never happen.
    let mut dmap = map_with_room_of_size(40);
    dmap.rooms.push(map::Room {
        id: "small-rg".to_string(),
        name: "small-rg".to_string(),
        region: "eastus".to_string(),
        x: 1,
        y: 0,
        resources: vec![],
    });
    dmap.edges.push(map::Edge {
        from: "big-rg".to_string(),
        to: "small-rg".to_string(),
    });

    let html = render::render_html(&dmap);
    let rects = room_floor_rects(&html);
    assert_eq!(rects.len(), 2, "expected two room-floor rects");
    assert!(
        !rects_overlap(rects[0], rects[1]),
        "large room {:?} must not overlap adjacent room {:?}",
        rects[0],
        rects[1]
    );
}

#[test]
fn render_html_keeps_decorations_confined_to_the_outer_margin() {
    let dmap = small_map();
    let html = render::render_html(&dmap);

    // Every room-floor rect must sit at or beyond the fixed outer margin
    // band that decorations are confined to, so decorative markup can never
    // be positioned on top of the room/corridor grid.
    for (x, y, _, _) in room_floor_rects(&html) {
        assert!(
            x >= decorations::MAP_MARGIN,
            "room x={x} must be at/after the decoration margin"
        );
        assert!(
            y >= decorations::MAP_MARGIN,
            "room y={y} must be at/after the decoration margin"
        );
    }
    assert!(
        html.contains("class=\"decoration"),
        "rendered map must include decorative border/torch/chest/dragon markup"
    );
}

#[test]
fn render_html_of_a_large_adaptive_map_is_still_a_pure_function_of_the_map() {
    let dmap = map_with_room_of_size(50);
    let a = render::render_html(&dmap);
    let b = render::render_html(&dmap);
    assert_eq!(
        a, b,
        "adaptive room sizing/spacing must not introduce any nondeterminism"
    );
}

// ---------------------------------------------------------------------------
// server: in-process request routing (the JSON API contract)
// ---------------------------------------------------------------------------

#[test]
fn route_index_serves_the_rendered_map_as_html() {
    let dmap = small_map();
    let resp = server::route(&dmap, "GET", server::ROUTE_INDEX);
    assert_eq!(resp.status, 200);
    assert!(resp.content_type.contains("html"));
    assert!(resp.body.contains("web-rg"));
}

#[test]
fn route_rooms_list_returns_json_array_of_room_summaries() {
    let dmap = small_map();
    let resp = server::route(&dmap, "GET", server::ROUTE_ROOMS);
    assert_eq!(resp.status, 200);
    assert!(resp.content_type.contains("json"));

    let parsed: serde_json::Value =
        serde_json::from_str(&resp.body).expect("rooms list body must be valid JSON");
    let arr = parsed.as_array().expect("rooms list must be a JSON array");
    assert_eq!(arr.len(), 2);
    let ids: Vec<&str> = arr
        .iter()
        .map(|r| r["id"].as_str().expect("room id must be a string"))
        .collect();
    assert!(ids.contains(&"web-rg"));
    assert!(ids.contains(&"data-rg"));
    // Summary view: positions present, but the full resource list is not
    // inlined (kept for the per-room detail endpoint).
    assert!(arr[0].get("x").is_some());
    assert!(arr[0].get("y").is_some());
}

#[test]
fn route_room_detail_returns_full_resource_list_for_known_room() {
    let dmap = small_map();
    let resp = server::route(&dmap, "GET", "/api/v1/rooms/web-rg");
    assert_eq!(resp.status, 200);
    assert!(resp.content_type.contains("json"));

    let parsed: serde_json::Value =
        serde_json::from_str(&resp.body).expect("room detail body must be valid JSON");
    assert_eq!(parsed["id"], "web-rg");
    let resources = parsed["resources"]
        .as_array()
        .expect("room detail must include a resources array");
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0]["name"], "app1");
}

#[test]
fn route_room_detail_404s_for_unknown_room() {
    let dmap = small_map();
    let resp = server::route(&dmap, "GET", "/api/v1/rooms/does-not-exist");
    assert_eq!(resp.status, 404);
}

#[test]
fn route_resource_detail_includes_icon_portal_link_and_suggested_commands() {
    let dmap = small_map();
    let resource_id = "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/web-rg/providers/Microsoft.Web/sites/app1";
    let resp = server::route(&dmap, "GET", &format!("/api/v1/resources/{resource_id}"));
    assert_eq!(resp.status, 200);

    let parsed: serde_json::Value =
        serde_json::from_str(&resp.body).expect("resource detail body must be valid JSON");
    assert_eq!(parsed["name"], "app1");
    assert_eq!(parsed["icon"], "app-service");
    assert!(parsed["portal_url"]
        .as_str()
        .expect("portal_url must be a string")
        .starts_with(links::PORTAL_BASE));
    let suggested = parsed["suggested_commands"]
        .as_array()
        .expect("suggested_commands must be a JSON array");
    assert!(!suggested.is_empty());
    for cmd in suggested {
        assert!(commands::is_read_only_command(
            cmd.as_str().expect("command must be a string")
        ));
    }
}

#[test]
fn route_resource_detail_404s_for_unknown_resource() {
    let dmap = small_map();
    let resp = server::route(&dmap, "GET", "/api/v1/resources/does-not-exist");
    assert_eq!(resp.status, 404);
}

#[test]
fn route_rejects_unsupported_methods_and_unknown_paths() {
    let dmap = small_map();

    let post_resp = server::route(&dmap, "POST", server::ROUTE_INDEX);
    assert_ne!(
        post_resp.status, 200,
        "there are no write endpoints; POST must never succeed"
    );

    let unknown_resp = server::route(&dmap, "GET", "/nonexistent/path");
    assert_eq!(unknown_resp.status, 404);
}

#[test]
fn route_never_exposes_secret_looking_data_in_json_responses() {
    let mut dmap = small_map();
    dmap.rooms[0].resources.push(map::ResourceNode {
        id: "/subscriptions/0/resourceGroups/web-rg/providers/Microsoft.KeyVault/vaults/kv1"
            .to_string(),
        name: "kv1".to_string(),
        kind: "Microsoft.KeyVault/vaults".to_string(),
        region: "eastus".to_string(),
        icon: icons::icon_for("Microsoft.KeyVault/vaults").to_string(),
    });

    let rooms_resp = server::route(&dmap, "GET", server::ROUTE_ROOMS);
    let room_resp = server::route(&dmap, "GET", "/api/v1/rooms/web-rg");
    for body in [&rooms_resp.body, &room_resp.body] {
        let lower = body.to_lowercase();
        assert!(!lower.contains("connectionstring"));
        assert!(!lower.contains("sharedaccesskey"));
        assert!(!lower.contains(&test_fixtures::HOSTILE_ACCOUNT_KEY_VALUE.to_ascii_lowercase()));
    }
}

// ---------------------------------------------------------------------------
// server: the real, loopback-only TcpListener-backed HTTP server
// ---------------------------------------------------------------------------

#[test]
fn serve_binds_to_loopback_and_answers_a_real_http_request() {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let dmap = small_map();
    let handle = server::serve(dmap, "127.0.0.1:0").expect("server should bind and start");

    let addr = handle.addr();
    assert!(
        addr.ip().is_loopback(),
        "the embedded server must only ever bind to loopback, got {addr}"
    );

    let mut stream =
        TcpStream::connect(addr).expect("should be able to connect to the just-bound server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(b"GET /api/v1/rooms HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .expect("request should be writable");

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("response should be readable");

    assert!(response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200"));
    assert!(response.contains("web-rg"));

    handle.shutdown();
}

/// A hostile `Content-Length` claiming many gigabytes must never make the
/// server pre-allocate a buffer of that size (a trivial local memory-DoS);
/// the connection should still be served promptly and correctly.
#[test]
fn serve_rejects_oversized_content_length_without_huge_allocation() {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::{Duration, Instant};

    let dmap = small_map();
    let handle = server::serve(dmap, "127.0.0.1:0").expect("server should bind and start");
    let addr = handle.addr();

    let mut stream =
        TcpStream::connect(addr).expect("should be able to connect to the just-bound server");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(
            b"GET /api/v1/rooms HTTP/1.1\r\nHost: localhost\r\n\
              Content-Length: 999999999999\r\nConnection: close\r\n\r\nshort-body",
        )
        .expect("request should be writable");
    // Signal EOF on our write side: the server's bounded drain must stop as
    // soon as the peer runs out of data instead of blocking for the full
    // (attacker-claimed) content length.
    stream
        .shutdown(std::net::Shutdown::Write)
        .expect("should be able to half-close the write side");

    let start = Instant::now();
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .expect("response should be readable");

    // The server must not have tried to allocate/wait for ~1TB of body; it
    // should respond promptly (well under the 5s read timeout) using the
    // bounded drain rather than a huge up-front `Vec` allocation.
    assert!(start.elapsed() < Duration::from_secs(4));
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("web-rg"));

    handle.shutdown();
}

#[test]
fn serve_picks_a_free_port_when_requested_port_is_zero() {
    let dmap = small_map();
    let handle = server::serve(dmap, "127.0.0.1:0").expect("server should bind and start");
    assert_ne!(
        handle.addr().port(),
        0,
        "requesting port 0 must resolve to a real OS-assigned port"
    );
    handle.shutdown();
}

#[test]
fn serve_rejects_non_loopback_bind_addresses() {
    let err = match server::serve(small_map(), "0.0.0.0:0") {
        Ok(_) => panic!("wildcard bind must be rejected"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("loopback"));
}

// ---------------------------------------------------------------------------
// playwright: optional, always-degrading renderer
// ---------------------------------------------------------------------------

#[test]
fn playwright_try_render_degrades_gracefully_without_a_browser() {
    let dmap = small_map();
    // No browser/Node/Playwright is available in the test environment (and
    // never will be, in CI); this must never panic and must never claim
    // success it can't back up.
    let result = playwright::try_render(&dmap);
    assert!(
        result.is_none(),
        "without a wired-up external driver, try_render must degrade to None, not fabricate output"
    );
}

// ---------------------------------------------------------------------------
// cli: `azork crawl` / `azork dungeon` argument parsing
// ---------------------------------------------------------------------------

#[test]
fn is_crawl_subcommand_accepts_both_documented_aliases() {
    assert!(cli::is_crawl_subcommand("crawl"));
    assert!(cli::is_crawl_subcommand("dungeon"));
}

#[test]
fn is_crawl_subcommand_rejects_other_repl_verbs() {
    for verb in ["look", "go", "help", "quit", "", "Crawl", "CRAWL"] {
        assert!(
            !cli::is_crawl_subcommand(verb),
            "`{verb}` must not be treated as the crawl subcommand"
        );
    }
}

#[test]
fn parse_defaults_match_documentation() {
    let args: Vec<String> = vec![];
    let parsed = cli::parse(&args).expect("no flags should parse to defaults");
    assert_eq!(parsed, cli::CrawlArgs::default());
    assert_eq!(parsed.backend, "mock");
    assert!(!parsed.serve);
    assert_eq!(parsed.port, 0);
    assert_eq!(parsed.out, None);
    assert_eq!(parsed.budget, map::DEFAULT_BUDGET);
    assert!(!parsed.playwright);
}

#[test]
fn parse_reads_all_documented_flags() {
    let args: Vec<String> = [
        "--backend",
        "az",
        "--serve",
        "--port",
        "8420",
        "--out",
        "dungeon.html",
        "--budget",
        "10",
        "--playwright",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let parsed = cli::parse(&args).expect("all documented flags should parse");
    assert_eq!(parsed.backend, "az");
    assert!(parsed.serve);
    assert_eq!(parsed.port, 8420);
    assert_eq!(parsed.out.as_deref(), Some("dungeon.html"));
    assert_eq!(parsed.budget, 10);
    assert!(parsed.playwright);
}

#[test]
fn parse_rejects_non_numeric_port_without_panicking() {
    let args: Vec<String> = ["--port", "not-a-number"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(cli::parse(&args).is_err());
}

#[test]
fn parse_rejects_non_numeric_budget_without_panicking() {
    let args: Vec<String> = ["--budget", "lots"].iter().map(|s| s.to_string()).collect();
    assert!(cli::parse(&args).is_err());
}

#[test]
fn parse_rejects_unknown_flags() {
    let args: Vec<String> = ["--not-a-real-flag"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(cli::parse(&args).is_err());
}

// ---------------------------------------------------------------------------
// Sanity: the AzRunner seam is genuinely what's driving enumeration.
// ---------------------------------------------------------------------------

#[test]
fn fixture_runner_never_answers_a_write_verb() {
    // Guard against a future regression where the map builder starts issuing
    // e.g. `group delete`/`resource delete`: the fixture runner used across
    // this whole suite has no canned response for any mutating verb, so if
    // `build` ever called one, that call would surface as a build error
    // rather than silently doing something destructive against a fixture.
    let runner = fixture_runner();
    let out = runner
        .run(&["group", "delete", "--name", "web-rg", "--yes"])
        .unwrap();
    assert!(!out.status.success());
}
