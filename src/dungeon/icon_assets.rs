//! Compile-time-embedded SVG icon bodies for every icon key in
//! [`crate::dungeon::type_table`].
//!
//! Each SVG is one of Microsoft's **official Azure Architecture Icons**
//! (the "Azure Public Service Icons" set published at
//! <https://learn.microsoft.com/en-us/azure/architecture/icons/>), used here
//! to illustrate this project's own architecture-diagram-style dungeon map,
//! per Microsoft's published icon terms — see
//! [`../../assets/azure-icons/LICENSE-NOTICE.md`] for the full attribution
//! and terms-of-use notice. Every file is embedded at compile time via
//! `include_str!` — never fetched or read from disk at runtime — so a saved
//! `--out` HTML document, and the served map, are fully self-contained and
//! work offline.

/// The single source of truth for known icon keys: each known key maps to
/// its own bundled SVG. Both [`svg_for`] and [`canonical_key`] are derived
/// from this table so the key list only has to be maintained in one place.
const ICONS: &[(&str, &str)] = &[
    ("storage-account", STORAGE_ACCOUNT),
    ("virtual-machine", VIRTUAL_MACHINE),
    ("app-service", APP_SERVICE),
    ("key-vault", KEY_VAULT),
    ("aks", AKS),
    ("sql-server", SQL_SERVER),
    ("cosmos-db", COSMOS_DB),
    ("virtual-network", VIRTUAL_NETWORK),
    ("public-ip", PUBLIC_IP),
    ("network-security-group", NETWORK_SECURITY_GROUP),
    ("load-balancer", LOAD_BALANCER),
    ("network-interface", NETWORK_INTERFACE),
    ("resource-group", RESOURCE_GROUP),
    ("mystery-chest", MYSTERY_CHEST),
];

/// Resolve an icon key (e.g. `"storage-account"`) to its inline SVG document.
/// Unknown/unmapped keys fall back to the bundled `mystery-chest` icon so a
/// newly-released or unrecognized Azure resource type never breaks
/// rendering.
pub fn svg_for(icon_key: &str) -> &'static str {
    ICONS
        .iter()
        .find(|(key, _)| *key == icon_key)
        .map(|(_, svg)| *svg)
        .unwrap_or(MYSTERY_CHEST)
}

/// Normalize an icon key to the one actually backing its rendered SVG
/// (i.e. what [`svg_for`] resolves it to): any key not in the known set
/// collapses to `"mystery-chest"`, so callers that key a deduplicated
/// `<symbol>` definition (e.g. `render.rs`) by this canonical key never
/// emit two different ids for what is, visually, the same fallback icon.
pub fn canonical_key(icon_key: &str) -> &'static str {
    ICONS
        .iter()
        .find(|(key, _)| *key == icon_key)
        .map(|(key, _)| *key)
        .unwrap_or("mystery-chest")
}

/// The `viewBox` attribute value declared on a bundled icon's outer `<svg>`
/// element (e.g. `"0 0 18 18"` for the official Azure Architecture Icons,
/// which all share that coordinate space). Used so the shared `<symbol>`
/// wrapping an icon's inner markup in `render.rs` declares a `viewBox`
/// matching the icon's *own* coordinate space — a mismatch here (e.g.
/// hardcoding `"0 0 24 24"` for an 18x18 icon) would silently crop or
/// mis-scale every icon on the rendered map. Falls back to the icon's
/// natural size if no `viewBox` attribute is present.
pub fn view_box(svg_key: &str) -> String {
    let svg = svg_for(svg_key);
    extract_view_box(svg).unwrap_or_else(|| "0 0 24 24".to_string())
}

fn extract_view_box(svg: &str) -> Option<String> {
    let needle = "viewBox=\"";
    let start = svg.find(needle)? + needle.len();
    let end = svg[start..].find('"')? + start;
    Some(svg[start..end].to_string())
}

/// Strip the outer `<svg ...> ... </svg>` wrapper from a bundled icon
/// document, leaving just its inner markup — used to nest an icon's shapes
/// inside a shared `<symbol>` definition (referenced via `<use>`) rather
/// than duplicating a full nested `<svg>` per resource instance.
pub fn inner_markup(svg_key: &str) -> String {
    let svg = svg_for(svg_key);
    let after_open = svg.find('>').map(|i| &svg[i + 1..]).unwrap_or(svg);
    after_open
        .rfind("</svg>")
        .map(|i| after_open[..i].trim())
        .unwrap_or(after_open)
        .to_string()
}

const STORAGE_ACCOUNT: &str = include_str!("../../assets/azure-icons/storage-account.svg");
const VIRTUAL_MACHINE: &str = include_str!("../../assets/azure-icons/virtual-machine.svg");
const APP_SERVICE: &str = include_str!("../../assets/azure-icons/app-service.svg");
const KEY_VAULT: &str = include_str!("../../assets/azure-icons/key-vault.svg");
const AKS: &str = include_str!("../../assets/azure-icons/aks.svg");
const SQL_SERVER: &str = include_str!("../../assets/azure-icons/sql-server.svg");
const COSMOS_DB: &str = include_str!("../../assets/azure-icons/cosmos-db.svg");
const VIRTUAL_NETWORK: &str = include_str!("../../assets/azure-icons/virtual-network.svg");
const PUBLIC_IP: &str = include_str!("../../assets/azure-icons/public-ip.svg");
const NETWORK_SECURITY_GROUP: &str =
    include_str!("../../assets/azure-icons/network-security-group.svg");
const LOAD_BALANCER: &str = include_str!("../../assets/azure-icons/load-balancer.svg");
const NETWORK_INTERFACE: &str = include_str!("../../assets/azure-icons/network-interface.svg");
const RESOURCE_GROUP: &str = include_str!("../../assets/azure-icons/resource-group.svg");
const MYSTERY_CHEST: &str = include_str!("../../assets/azure-icons/mystery-chest.svg");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_key_falls_back_to_mystery_chest() {
        assert_eq!(svg_for("nonexistent"), MYSTERY_CHEST);
    }

    #[test]
    fn every_bundled_icon_is_well_formed() {
        for (key, _) in ICONS {
            let svg = svg_for(key);
            assert!(svg.contains("<svg"));
            assert!(svg.contains("</svg>"));
        }
    }

    #[test]
    fn every_bundled_icon_declares_a_view_box() {
        // The official Azure Architecture Icons all share an "0 0 18 18"
        // coordinate space; asserting this catches an icon file that was
        // swapped in without checking it matches the rest of the set (which
        // would otherwise silently mis-scale on the rendered map).
        for (key, _) in ICONS {
            assert_eq!(
                view_box(key),
                "0 0 18 18",
                "icon {key} has an unexpected viewBox"
            );
        }
    }

    #[test]
    fn unknown_key_view_box_falls_back_to_mystery_chest() {
        assert_eq!(view_box("nonexistent"), view_box("mystery-chest"));
    }
}
