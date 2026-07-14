//! Optional real backend that shells out to the installed `az` CLI.
//!
//! This maps your actual Azure subscription into the dungeon. It is never used
//! by default and is never exercised by the test suite — the game must run with
//! zero credentials. Enable it with `--backend az` or `AZORK_BACKEND=az`.
//!
//! To avoid a JSON-parsing dependency we ask `az` for tab-separated output
//! (`-o tsv`) with narrow `--query` projections and parse the plain text.

use super::Backend;
use crate::parser::Direction;
use crate::world::{Resource, Room, World};
use std::process::Command;

/// Backend that queries the real Azure control plane via `az`.
pub struct AzBackend;

impl AzBackend {
    pub fn new() -> AzBackend {
        AzBackend
    }

    /// Run an `az` invocation and return stdout as a string.
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let output = Command::new("az")
            .args(args)
            .output()
            .map_err(|e| format!("failed to launch 'az' (is it installed & on PATH?): {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "'az {}' failed: {}",
                args.join(" "),
                stderr.trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl Default for AzBackend {
    fn default() -> Self {
        AzBackend::new()
    }
}

impl Backend for AzBackend {
    fn name(&self) -> &str {
        "az (live Azure)"
    }

    fn build_world(&self) -> Result<World, String> {
        // Current subscription name (best-effort).
        let subscription = self
            .run(&["account", "show", "--query", "name", "-o", "tsv"])
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "unknown-subscription".to_string());

        // Resource groups become rooms.
        let groups_raw = self.run(&[
            "group",
            "list",
            "--query",
            "[].{name:name,location:location}",
            "-o",
            "tsv",
        ])?;

        let mut groups: Vec<(String, String)> = Vec::new();
        for line in groups_raw.lines() {
            let mut cols = line.split('\t');
            if let Some(name) = cols.next() {
                let loc = cols.next().unwrap_or("unknown").to_string();
                if !name.trim().is_empty() {
                    groups.push((name.trim().to_string(), loc.trim().to_string()));
                }
            }
        }

        if groups.is_empty() {
            return Err(
                "no resource groups found (or not logged in). Try 'az login', or run with the \
                 default mock backend."
                    .to_string(),
            );
        }

        // Build rooms, chaining them north<->south so the estate is navigable.
        let mut rooms: Vec<Room> = Vec::new();
        for (i, (gname, location)) in groups.iter().enumerate() {
            let mut room = Room::new(
                gname,
                &format!("Resource group '{}' in {}.", gname, location),
                location,
                true, // assume monitored; we can't cheaply prove otherwise
            );
            if i > 0 {
                room = room.with_exit(Direction::South, &groups[i - 1].0);
            }
            if i + 1 < groups.len() {
                room = room.with_exit(Direction::North, &groups[i + 1].0);
            }

            // Resources in this group become objects.
            if let Ok(res_raw) = self.run(&[
                "resource",
                "list",
                "-g",
                gname,
                "--query",
                "[].{name:name,type:type}",
                "-o",
                "tsv",
            ]) {
                for line in res_raw.lines() {
                    let mut cols = line.split('\t');
                    if let Some(rname) = cols.next() {
                        let rtype = cols.next().unwrap_or("resource").to_string();
                        if !rname.trim().is_empty() {
                            room = room.with_resource(Resource::new(
                                rname.trim(),
                                rtype.trim(),
                                &format!("A live {} named {}.", rtype.trim(), rname.trim()),
                            ));
                        }
                    }
                }
            }

            rooms.push(room);
        }

        let start = rooms[0].name.clone();
        Ok(World::new(rooms, &start, &subscription))
    }
}
