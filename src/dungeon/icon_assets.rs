//! Compile-time-embedded SVG icon bodies for every icon key in
//! [`crate::dungeon::type_table`].
//!
//! Each SVG is an **original, hand-authored monochrome line glyph** owned by
//! this project — not Microsoft's copyrighted Azure Architecture Icons
//! artwork (see [`../../assets/azure-icons/LICENSE-NOTICE.md`] for the full
//! rationale: Microsoft's published terms permit *using* their icons to
//! illustrate your own diagram, but do not grant a license to redistribute
//! the icon files themselves bundled inside a third-party repository or
//! compiled into a third-party binary). Every file is embedded at compile
//! time via `include_str!` — never read from disk at runtime — so a saved
//! `--out` HTML document, and the served map, are fully self-contained and
//! work offline.

/// Resolve an icon key (e.g. `"storage-account"`) to its inline SVG document.
/// Unknown/unmapped keys fall back to the bundled `mystery-chest` icon so a
/// newly-released or unrecognized Azure resource type never breaks
/// rendering.
pub fn svg_for(icon_key: &str) -> &'static str {
    match icon_key {
        "storage-account" => STORAGE_ACCOUNT,
        "virtual-machine" => VIRTUAL_MACHINE,
        "app-service" => APP_SERVICE,
        "key-vault" => KEY_VAULT,
        "aks" => AKS,
        "sql-server" => SQL_SERVER,
        "cosmos-db" => COSMOS_DB,
        "virtual-network" => VIRTUAL_NETWORK,
        "public-ip" => PUBLIC_IP,
        "network-security-group" => NETWORK_SECURITY_GROUP,
        "load-balancer" => LOAD_BALANCER,
        "network-interface" => NETWORK_INTERFACE,
        "resource-group" => RESOURCE_GROUP,
        _ => MYSTERY_CHEST,
    }
}

/// Normalize an icon key to the one actually backing its rendered SVG
/// (i.e. what [`svg_for`] resolves it to): any key not in the known set
/// collapses to `"mystery-chest"`, so callers that key a deduplicated
/// `<symbol>` definition (e.g. `render.rs`) by this canonical key never
/// emit two different ids for what is, visually, the same fallback icon.
pub fn canonical_key(icon_key: &str) -> &'static str {
    match icon_key {
        "storage-account" => "storage-account",
        "virtual-machine" => "virtual-machine",
        "app-service" => "app-service",
        "key-vault" => "key-vault",
        "aks" => "aks",
        "sql-server" => "sql-server",
        "cosmos-db" => "cosmos-db",
        "virtual-network" => "virtual-network",
        "public-ip" => "public-ip",
        "network-security-group" => "network-security-group",
        "load-balancer" => "load-balancer",
        "network-interface" => "network-interface",
        "resource-group" => "resource-group",
        "mystery-chest" => "mystery-chest",
        _ => "mystery-chest",
    }
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
        for key in [
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
            "mystery-chest",
        ] {
            let svg = svg_for(key);
            assert!(svg.contains("<svg"));
            assert!(svg.contains("</svg>"));
        }
    }
}
