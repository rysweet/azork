//! The single Azure resource type -> (icon, suggested `az show` command)
//! table that [`crate::dungeon::icons::icon_for`] and
//! [`crate::dungeon::commands::suggested_commands`] are both derived from,
//! so the icon and the command shown for a given type can never drift out of
//! sync (see `docs/DUNGEON-CRAWLER.md#suggested-az-commands`).
//!
//! Each row is `(type_prefix, icon_key, command_template)`. `type_prefix` is
//! matched case-insensitively against the start of a resource's `type`
//! field (an exact match for the concrete types listed here, but a prefix
//! match keeps the table forward-compatible with sibling types under the
//! same provider namespace). `command_template` uses `{id}` as the
//! substitution point for the resource's full ARM id.

/// One row of the type -> (icon, command) table.
pub struct TypeEntry {
    pub type_prefix: &'static str,
    pub icon: &'static str,
    pub command_template: &'static str,
}

/// The full table, ordered from most to least specific (irrelevant today
/// since every prefix is an exact resource type, but kept in mind for future
/// additions of genuinely-prefix entries).
pub const TABLE: &[TypeEntry] = &[
    TypeEntry {
        type_prefix: "microsoft.storage/storageaccounts",
        icon: "storage-account",
        command_template: "az storage account show --ids {id}",
    },
    TypeEntry {
        type_prefix: "microsoft.compute/virtualmachines",
        icon: "virtual-machine",
        command_template: "az vm show --ids {id}",
    },
    TypeEntry {
        type_prefix: "microsoft.web/sites",
        icon: "app-service",
        command_template: "az webapp show --ids {id}",
    },
    TypeEntry {
        type_prefix: "microsoft.keyvault/vaults",
        icon: "key-vault",
        command_template: "az keyvault show --ids {id}",
    },
    TypeEntry {
        type_prefix: "microsoft.containerservice/managedclusters",
        icon: "aks",
        command_template: "az aks show --ids {id}",
    },
    TypeEntry {
        type_prefix: "microsoft.sql/servers",
        icon: "sql-server",
        command_template: "az sql server show --ids {id}",
    },
    TypeEntry {
        type_prefix: "microsoft.documentdb/databaseaccounts",
        icon: "cosmos-db",
        command_template: "az cosmosdb show --ids {id}",
    },
    TypeEntry {
        type_prefix: "microsoft.network/virtualnetworks",
        icon: "virtual-network",
        command_template: "az network vnet show --ids {id}",
    },
    TypeEntry {
        type_prefix: "microsoft.network/publicipaddresses",
        icon: "public-ip",
        command_template: "az network public-ip show --ids {id}",
    },
    TypeEntry {
        type_prefix: "microsoft.network/networksecuritygroups",
        icon: "network-security-group",
        command_template: "az network nsg show --ids {id}",
    },
    TypeEntry {
        type_prefix: "microsoft.network/loadbalancers",
        icon: "load-balancer",
        command_template: "az network lb show --ids {id}",
    },
    TypeEntry {
        type_prefix: "microsoft.network/networkinterfaces",
        icon: "network-interface",
        command_template: "az network nic show --ids {id}",
    },
    TypeEntry {
        type_prefix: "microsoft.resources/resourcegroups",
        icon: "resource-group",
        command_template: "az group show --name {id}",
    },
];

/// Icon used when no row in [`TABLE`] matches.
pub const DEFAULT_ICON: &str = "mystery-chest";

/// Command template used when no row in [`TABLE`] matches.
pub const DEFAULT_COMMAND_TEMPLATE: &str = "az resource show --ids {id}";

/// Look up the table row matching `resource_type` (case-insensitive), if any.
pub fn lookup(resource_type: &str) -> Option<&'static TypeEntry> {
    let needle = resource_type.to_lowercase();
    TABLE.iter().find(|entry| needle == entry.type_prefix)
}
