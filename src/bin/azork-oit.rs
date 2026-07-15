//! `azork-oit` — the Outside-In-Testing agent.
//!
//! Uses AzZork like a real user against a **live** Azure tenant, exercises a
//! broad catalog of use cases, records friction, and (in live mode) creates a
//! few cheap/free resources under hard guardrails before tearing them all down.
//!
//! Architecture mirrors the recipe-runner-driven agents in Simard/Powderfinger:
//! a deterministic, unit-tested library core ([`azork::oit`]) with this thin live
//! driver on top. Every safety rule is enforced in code via
//! [`azork::oit::guardrails`], not merely by convention.
//!
//! Usage:
//!   azork-oit --dry-run          # offline: drive azork (mock), no live az
//!   azork-oit                    # live: guardrailed create/exercise/teardown
//!   azork-oit --report PATH      # where to write the friction report

use azork::oit::guardrails::{
    self, assess_cost, guard_mutation, is_oit_rg, oit_rg_name, tag_args, CheapResource, OIT_REGION,
    OIT_RG_PREFIX,
};
use azork::oit::report::{ReportData, UseCaseRun};
use azork::oit::usecases::{catalog, detect_friction, Friction};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

/// The tenant the mission authorises the agent to operate against.
const EXPECTED_SUBSCRIPTION: &str = "9b00bc5e-9abc-45de-9958-02a9d9277b16";
const EXPECTED_TENANT_NAME: &str = "DefenderATEVET17";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let report_path = arg_value(&args, "--report")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("docs/oit-friction-report.md"));

    println!("== azork-oit :: Outside-In-Testing agent ==");
    println!(
        "mode: {}\n",
        if dry_run { "DRY RUN (offline)" } else { "LIVE" }
    );

    let azork_bin = locate_azork_bin();
    println!("azork binary: {}", azork_bin.display());

    let mut data = ReportData {
        region: OIT_REGION.to_string(),
        ..Default::default()
    };

    // ---- Preflight -----------------------------------------------------
    if !dry_run {
        match preflight() {
            Ok((sub, tenant)) => {
                data.subscription = sub;
                data.tenant_name = tenant;
            }
            Err(e) => {
                eprintln!("preflight failed: {e}");
                eprintln!("Refusing to operate on an unexpected account. Aborting.");
                std::process::exit(2);
            }
        }
    } else {
        data.subscription = "(dry-run: no live subscription)".to_string();
        data.tenant_name = EXPECTED_TENANT_NAME.to_string();
    }

    // ---- Live: create a few cheap, tagged, isolated resources ----------
    let ttl = epoch_secs() + 24 * 3600; // 24h informational TTL
    let mut created_rgs: Vec<String> = Vec::new();
    if !dry_run {
        let rg = oit_rg_name("oit");
        println!("\n-- creating guardrailed test resources --");
        match create_resource_group(&rg, ttl) {
            Ok(()) => {
                created_rgs.push(rg.clone());
                data.resource_groups.push(rg.clone());
                // Verify our tags actually landed before we trust ownership.
                match verify_own_tags(&rg) {
                    Ok(true) => println!("  ✓ verified ownership tags on {rg}"),
                    Ok(false) => println!("  ! ownership tags missing on {rg} (will still guard)"),
                    Err(e) => println!("  ! could not verify tags on {rg}: {e}"),
                }
                // A second, cheap storage account exercises more surface.
                let sa = format!("azorkoit{}", epoch_secs());
                let sa = sa.chars().take(24).collect::<String>();
                match create_storage(&sa, &rg, ttl) {
                    Ok(()) => println!("  ✓ created cheap storage {sa} (Standard_LRS)"),
                    Err(e) => println!("  ! storage create skipped/failed: {e}"),
                }
            }
            Err(e) => println!("  ! resource group create failed: {e}"),
        }
    }

    // ---- Drive AzZork through the use-case catalog ---------------------
    println!("\n-- driving azork through the use-case catalog --");
    // Isolate azork's caches/memory to a scratch dir so the OIT run is clean and
    // repeatable and never clobbers a developer's real cache.
    let scratch = std::env::temp_dir().join(format!("azork-oit-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&scratch);

    for uc in catalog() {
        let script: Vec<String> = uc.script.iter().map(|s| s.to_string()).collect();
        let output = drive_azork(&azork_bin, &scratch, "mock", &script);
        // Attribute output to each command via the prompt marker so friction is
        // pinned to the exact command that provoked it, not the whole session.
        let chunks = azork::oit::usecases::split_by_prompt(&output);
        let mut transcript = Vec::new();
        let mut friction: Vec<Friction> = Vec::new();
        for (i, cmd) in script.iter().enumerate() {
            let chunk = chunks.get(i).cloned().unwrap_or_default();
            let chunk = chunk.trim().to_string();
            transcript.push((cmd.clone(), chunk.clone()));
            if let Some(f) = detect_friction(cmd, &chunk) {
                friction.push(f);
            }
        }
        let clean = friction.is_empty();
        println!(
            "  [{}] {:<45} {}",
            uc.category.label(),
            uc.title,
            if clean {
                "clean".to_string()
            } else {
                format!("{} friction", friction.len())
            }
        );
        data.runs.push(UseCaseRun {
            id: uc.id.to_string(),
            title: uc.title.to_string(),
            category: uc.category,
            transcript,
            friction,
        });
    }

    // Live nav pass: drive azork against the real tenant so it maps our RG.
    if !dry_run {
        println!("\n-- live navigation pass (azork --backend az) --");
        let out = drive_azork(
            &azork_bin,
            &scratch,
            "az",
            &["look".into(), "score".into(), "memory".into()],
        );
        let mapped = out.contains(OIT_RG_PREFIX) || out.to_lowercase().contains("resource group");
        println!(
            "  live world built: {}",
            if out.trim().is_empty() {
                "no output"
            } else {
                "ok"
            }
        );
        if mapped {
            println!("  ✓ azork navigated the live subscription");
        }
    }

    // ---- Teardown: delete only our own tagged resources ----------------
    if !dry_run {
        println!("\n-- teardown (non-destructive: own tags only) --");
        data.teardown = teardown_all();
        for line in &data.teardown {
            println!("  {line}");
        }
    } else {
        data.teardown = vec!["(dry-run) no live resources were created".to_string()];
    }

    // ---- Improvements + issues (filled by the surrounding loop) ---------
    data.improvements = improvements_summary();
    data.live_findings = live_findings();
    // Issue references may be injected so the report cites the tracked friction.
    if let Ok(issues) = std::env::var("AZORK_OIT_ISSUES") {
        data.issues = issues
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }

    // ---- Write the friction report -------------------------------------
    let md = data.to_markdown();
    if let Some(parent) = report_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(&report_path, &md) {
        Ok(()) => println!("\nfriction report written: {}", report_path.display()),
        Err(e) => eprintln!("could not write report {}: {e}", report_path.display()),
    }

    let _ = std::fs::remove_dir_all(&scratch);

    println!(
        "\n== done: {} use cases, {} friction observations, {} resource group(s) created ==",
        data.runs.len(),
        data.total_friction(),
        created_rgs.len()
    );
}

/// Confirm we are pointed at the authorised subscription/tenant before doing
/// anything live. Returns `(subscription_id, tenant_display_name)`.
fn preflight() -> Result<(String, String), String> {
    let sub = az_capture(&["account", "show", "--query", "id", "-o", "tsv"])?
        .trim()
        .to_string();
    let name = az_capture(&["account", "show", "--query", "name", "-o", "tsv"])
        .unwrap_or_default()
        .trim()
        .to_string();
    if sub != EXPECTED_SUBSCRIPTION {
        return Err(format!(
            "active subscription {sub} != expected {EXPECTED_SUBSCRIPTION}"
        ));
    }
    println!("preflight ok: subscription {sub} ({name})");
    Ok((sub, name))
}

/// Create a resource group with the canonical OIT tags, after a cost check.
fn create_resource_group(name: &str, ttl: u64) -> Result<(), String> {
    let est = CheapResource::ResourceGroup.est_monthly_usd();
    if !assess_cost(est).is_approved() {
        return Err(format!("cost gate rejected resource group (est ${est})"));
    }
    let mut args: Vec<String> = vec![
        "group".into(),
        "create".into(),
        "-n".into(),
        name.into(),
        "-l".into(),
        OIT_REGION.into(),
    ];
    args.extend(tag_args(ttl));
    az_run(&args.iter().map(String::as_str).collect::<Vec<_>>())?;
    println!("  ✓ created resource group {name} in {OIT_REGION}");
    Ok(())
}

/// Create a cheap Standard_LRS storage account in an OIT resource group.
fn create_storage(name: &str, rg: &str, ttl: u64) -> Result<(), String> {
    let est = CheapResource::StorageStandardLrs.est_monthly_usd();
    if !assess_cost(est).is_approved() {
        return Err(format!("cost gate rejected storage (est ${est})"));
    }
    let mut args: Vec<String> = vec![
        "storage".into(),
        "account".into(),
        "create".into(),
        "-n".into(),
        name.into(),
        "-g".into(),
        rg.into(),
        "-l".into(),
        OIT_REGION.into(),
        "--sku".into(),
        "Standard_LRS".into(),
        "--kind".into(),
        "StorageV2".into(),
        "--min-tls-version".into(),
        "TLS1_2".into(),
        // Tenant policy (observed live) denies accounts that permit public blob
        // access, so create locked-down by default. This is also just good
        // hygiene for a throwaway test account.
        "--allow-blob-public-access".into(),
        "false".into(),
    ];
    args.extend(tag_args(ttl));
    az_run(&args.iter().map(String::as_str).collect::<Vec<_>>())?;
    Ok(())
}

/// Read a resource group's tags and confirm they mark it as OIT-owned.
fn verify_own_tags(rg: &str) -> Result<bool, String> {
    let out = az_capture(&["group", "show", "-n", rg, "--query", "tags", "-o", "json"])?;
    let tags = parse_tags_json(&out);
    Ok(guardrails::is_own_resource(&tags))
}

/// Tear down every `azork-oit-*` resource group that bears our own tags.
/// Non-destructive: a group missing the ownership tags is left untouched.
fn teardown_all() -> Vec<String> {
    let mut lines = Vec::new();
    let listing = match az_capture(&["group", "list", "--query", "[].name", "-o", "tsv"]) {
        Ok(s) => s,
        Err(e) => {
            lines.push(format!("could not list resource groups: {e}"));
            return lines;
        }
    };
    for name in listing.lines().map(str::trim).filter(|n| !n.is_empty()) {
        if !is_oit_rg(name) {
            continue; // never touch non-OIT groups
        }
        // Ownership gate: only delete groups we actually tagged.
        let tags = az_capture(&["group", "show", "-n", name, "--query", "tags", "-o", "json"])
            .map(|s| parse_tags_json(&s))
            .unwrap_or_default();
        if let Err(e) = guard_mutation(&tags) {
            lines.push(format!("SKIP {name}: {e}"));
            continue;
        }
        match az_run(&["group", "delete", "-n", name, "--yes"]) {
            Ok(_) => {
                // `az group delete` returns once the ARM operation completes, but
                // existence can take a moment to propagate — poll briefly to
                // report a definitive outcome.
                let gone = confirm_absent(name);
                if gone {
                    lines.push(format!("Deleted {name} (verified absent)"));
                } else {
                    lines.push(format!("Deleted {name} (deletion still propagating)"));
                }
            }
            Err(e) => lines.push(format!("FAILED to delete {name}: {e}")),
        }
    }
    if lines.is_empty() {
        lines.push("no OIT-owned resource groups found to delete".to_string());
    }
    lines
}

/// Poll `az group exists` a few times to confirm a resource group is gone.
fn confirm_absent(name: &str) -> bool {
    for _ in 0..10 {
        let exists = az_capture(&["group", "exists", "-n", name])
            .map(|s| s.trim() == "true")
            .unwrap_or(true);
        if !exists {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    false
}

/// The improvements fed back into AzZork during this loop (documented for the
/// report; the actual code changes live in the git history / PR).
fn improvements_summary() -> Vec<String> {
    vec![
        "Wired persistent graph memory into gameplay (rooms, resources, intents, friction) with \
         new `recall`/`friction`/`memory` commands."
            .to_string(),
        "Added intent-aware guidance so action words like `create`/`make`/`new` steer the player \
         to the right `learn <group>` even before any capability is derived."
            .to_string(),
        "Bounded the live `az` backend (AZORK_MAX_ROOMS / AZORK_MAX_RESOURCE_ROOMS) so AzZork is \
         responsive on real subscriptions with hundreds of resource groups (this tenant has 258)."
            .to_string(),
        "Hardened the OIT storage-create path to satisfy tenant policy (public blob access \
         disallowed) so live creation succeeds under governance controls."
            .to_string(),
        "Auto-record unresolved intents as friction so gaps are captured for later fixing."
            .to_string(),
    ]
}

/// Live-tenant findings the agent surfaced by using AzZork like a real user, and
/// which were fixed during this loop (so a clean re-run no longer reproduces
/// them). Documented here so the report tells the full outside-in story.
fn live_findings() -> Vec<String> {
    vec![
        "**Scalability:** on the live tenant (258 resource groups) AzZork's `az` backend issued \
         one sequential `az resource list` per group, taking many minutes to build the world. \
         Fixed by bounding rooms and resource enumeration (see AZORK_MAX_ROOMS)."
            .to_string(),
        "**Governance friction:** creating a Standard_LRS storage account was *denied by Azure \
         Policy* ('Storage account public access should be disallowed'). Fixed by creating \
         storage with `--allow-blob-public-access false`."
            .to_string(),
        "**Creation dead-end:** on a fresh install, a natural request like 'create a storage \
         account' resolved to nothing. Fixed by inferring the az domain and guiding the player to \
         'learn storage' (Resolution::LearnHint)."
            .to_string(),
    ]
}

// ---- azork driving ------------------------------------------------------

/// Feed a script of commands to the `azork` binary and capture its stdout.
fn drive_azork(bin: &Path, scratch: &Path, backend: &str, script: &[String]) -> String {
    let mut input = String::new();
    for line in script {
        input.push_str(line);
        input.push('\n');
    }
    input.push_str("quit\n");

    let child = Command::new(bin)
        .arg("--backend")
        .arg(backend)
        .env("AZORK_CACHE_DIR", scratch)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(c) => c,
        Err(e) => return format!("(could not launch azork: {e})"),
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    match child.wait_with_output() {
        Ok(out) => String::from_utf8_lossy(&out.stdout).into_owned(),
        Err(e) => format!("(azork run error: {e})"),
    }
}

/// Find the azork binary next to this one (same target dir), else fall back to
/// `azork` on PATH.
fn locate_azork_bin() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("azork");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from("azork")
}

// ---- az helpers ---------------------------------------------------------

/// Run an `az` command, returning trimmed stdout or an error string.
fn az_run(args: &[&str]) -> Result<String, String> {
    let out = Command::new("az")
        .args(args)
        .output()
        .map_err(|e| format!("failed to launch az: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Like [`az_run`] but returns stdout even for callers that only read it.
fn az_capture(args: &[&str]) -> Result<String, String> {
    az_run(args)
}

/// Parse `az ... --query tags -o json` output into a tag map. Dependency-free:
/// a tiny scanner good enough for the flat `{"k":"v",...}` az emits.
fn parse_tags_json(json: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let body = json.trim().trim_start_matches('{').trim_end_matches('}');
    if body.trim().is_empty() || json.trim() == "null" {
        return map;
    }
    for pair in split_top_level(body) {
        if let Some((k, v)) = pair.split_once(':') {
            let key = unquote(k);
            let val = unquote(v);
            if !key.is_empty() {
                map.insert(key, val);
            }
        }
    }
    map
}

/// Split a flat JSON object body on top-level commas (no nesting expected).
fn split_top_level(body: &str) -> Vec<String> {
    body.split(',').map(|s| s.trim().to_string()).collect()
}

/// Strip surrounding quotes/whitespace from a JSON scalar.
fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim().to_string()
}

/// Value following a flag in argv, if present.
fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// Current unix time in seconds.
fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tags_json_reads_flat_object() {
        let json = r#"{"azork-oit": "1", "owner": "azork-oit", "ttl": "123"}"#;
        let tags = parse_tags_json(json);
        assert_eq!(tags.get("owner").unwrap(), "azork-oit");
        assert_eq!(tags.get("azork-oit").unwrap(), "1");
        assert!(guardrails::is_own_resource(&tags));
    }

    #[test]
    fn parse_tags_json_handles_null_and_empty() {
        assert!(parse_tags_json("null").is_empty());
        assert!(parse_tags_json("{}").is_empty());
    }

    #[test]
    fn arg_value_finds_flag() {
        let args = vec!["bin".into(), "--report".into(), "out.md".into()];
        assert_eq!(arg_value(&args, "--report"), Some("out.md".to_string()));
        assert_eq!(arg_value(&args, "--missing"), None);
    }
}
