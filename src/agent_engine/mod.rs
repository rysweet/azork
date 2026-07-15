//! Embedded agentic recipe-running capability.
//!
//! This module embeds the [`recipe-runner-rs`] engine directly into AzZork's
//! default build (via a vendored, offline `path` dependency — see
//! `vendor/recipe-runner-rs/`). It implements the runner's
//! [`Adapter`](recipe_runner_rs::adapters::Adapter) trait, then hands it to
//! `run_recipe`, letting an amplihack recipe orchestrate multi-step intent
//! resolution (agent steps + bash steps) around AzZork's own learned
//! [`CapabilityRegistry`](crate::capabilities::CapabilityRegistry).
//!
//! It is deterministic where it can be: an *agent step* resolves its prompt
//! against the learned registry via AzZork's offline resolver (no LLM, no
//! network), while *bash steps* are delegated to the runner's standard CLI
//! subprocess adapter so recipes can still shell out to `az` for real work.
//!
//! Because `recipe-runner-rs` is vendored offline and depended on directly by
//! this crate, `cargo build`/`cargo test` at the repo root compile and
//! exercise this capability by default — no separate opt-in crate or
//! side-by-side sibling checkout is required.
//!
//! [`recipe-runner-rs`]: ../../vendor/recipe-runner-rs/

use std::collections::HashMap;

use recipe_runner_rs::adapters::cli_subprocess::CLISubprocessAdapter;
use recipe_runner_rs::adapters::Adapter as RecipeAdapter;
use recipe_runner_rs::models::RecipeResult;
use recipe_runner_rs::run_recipe;

use crate::agent::{Adapter, MockAdapter};
use crate::capabilities::CapabilityRegistry;

/// Embeds AzZork into the `recipe-runner-rs` [`RecipeAdapter`] seam.
///
/// * Agent steps → resolve the prompt against learned capabilities using
///   AzZork's deterministic offline resolver (so the default agentic path stays
///   reproducible and network-free).
/// * Bash steps → delegate to the runner's [`CLISubprocessAdapter`], letting a
///   recipe shell out (e.g. to `az`) when it genuinely needs to.
pub struct AzorkAdapter {
    registry: CapabilityRegistry,
    bash: CLISubprocessAdapter,
    resolver: MockAdapter,
}

impl AzorkAdapter {
    /// Build an adapter over a snapshot of the learned capability registry.
    pub fn new(registry: CapabilityRegistry) -> AzorkAdapter {
        AzorkAdapter {
            registry,
            bash: CLISubprocessAdapter::new(),
            resolver: MockAdapter::new(),
        }
    }
}

impl RecipeAdapter for AzorkAdapter {
    fn execute_agent_step(
        &self,
        prompt: &str,
        _agent_name: Option<&str>,
        _system_prompt: Option<&str>,
        _mode: Option<&str>,
        _working_dir: &str,
        _model: Option<&str>,
        _timeout: Option<u64>,
    ) -> Result<String, anyhow::Error> {
        // Resolve the intent against what AzZork has learned so far. This is the
        // offline, deterministic resolution the game uses at the prompt — surfaced
        // here so a recipe can compose it with other steps.
        Ok(self.resolver.resolve(prompt, &self.registry).narrate())
    }

    fn execute_bash_step(
        &self,
        command: &str,
        working_dir: &str,
        timeout: Option<u64>,
        extra_env: &HashMap<String, String>,
    ) -> Result<String, anyhow::Error> {
        self.bash
            .execute_bash_step(command, working_dir, timeout, extra_env)
    }

    fn is_available(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        "azork"
    }
}

/// Run an inline amplihack recipe with AzZork as the adapter.
///
/// `dry_run` executes the recipe's control flow without invoking real bash
/// commands, which keeps tests hermetic. Returns the runner's structured
/// [`RecipeResult`].
pub fn run_intent_recipe(
    yaml: &str,
    registry: CapabilityRegistry,
    dry_run: bool,
) -> Result<RecipeResult, String> {
    run_recipe(yaml, AzorkAdapter::new(registry), None, dry_run).map_err(|e| e.to_string())
}

/// A minimal built-in recipe that resolves a free-text intent into an AzZork
/// narration via a single agent step.
pub const INTENT_RESOLUTION_RECIPE: &str = r#"
name: azork-intent-resolution
description: Resolve an ambiguous azork intent against learned az capabilities.
steps:
  - id: resolve
    type: agent
    agent: azork
    prompt: "Resolve this Azure intent into an azork rite: {{ intent }}"
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::Capability;

    fn registry() -> CapabilityRegistry {
        let mut reg = CapabilityRegistry::new();
        reg.insert(Capability::new(
            "storage",
            "create",
            "Create a storage account.",
            None,
        ));
        reg
    }

    #[test]
    fn adapter_agent_step_resolves_via_registry() {
        let a = AzorkAdapter::new(registry());
        let out = a
            .execute_agent_step(
                "create a storage account",
                None,
                None,
                None,
                ".",
                None,
                None,
            )
            .expect("agent step");
        assert!(!out.is_empty());
    }

    #[test]
    fn adapter_metadata() {
        let a = AzorkAdapter::new(registry());
        assert_eq!(a.name(), "azork");
        assert!(a.is_available());
    }

    #[test]
    fn dry_run_recipe_executes_offline() {
        // dry_run keeps this hermetic: no bash, no network.
        let res = run_intent_recipe(INTENT_RESOLUTION_RECIPE, registry(), true)
            .expect("recipe should parse and run");
        assert_eq!(res.recipe_name, "azork-intent-resolution");
    }

    #[test]
    fn embedded_by_default_build_compiles_and_runs() {
        // Regression test proving the agentic capability is part of the default
        // build (no feature flag, no separate crate): if this test compiles and
        // passes under plain `cargo test`, `agent_engine` is embedded, not opt-in.
        assert!(!INTENT_RESOLUTION_RECIPE.is_empty());
        let res = run_intent_recipe(INTENT_RESOLUTION_RECIPE, registry(), true)
            .expect("embedded recipe engine should run offline by default");
        assert!(res.success);
    }

    #[test]
    fn adapter_agent_step_handles_unrecognized_intent() {
        // Edge case: an intent that matches nothing in the registry should still
        // resolve to a non-empty narration (the offline resolver's fallback),
        // never an error or an empty string.
        let a = AzorkAdapter::new(registry());
        let out = a
            .execute_agent_step(
                "summon a dragon from the void",
                None,
                None,
                None,
                ".",
                None,
                None,
            )
            .expect("agent step should not error on unknown intent");
        assert!(!out.is_empty());
    }

    #[test]
    fn adapter_agent_step_resolves_with_empty_registry() {
        // Edge case: an empty capability registry (nothing learned yet) must not
        // panic or error; the resolver should still produce a narration.
        let a = AzorkAdapter::new(CapabilityRegistry::new());
        let out = a
            .execute_agent_step(
                "create a storage account",
                None,
                None,
                None,
                ".",
                None,
                None,
            )
            .expect("agent step should not error with an empty registry");
        assert!(!out.is_empty());
    }

    #[test]
    fn run_intent_recipe_rejects_malformed_yaml() {
        // Error handling: malformed recipe YAML must surface as an Err, not
        // panic, and the error message should be non-empty for diagnosis.
        let bad_yaml = "not: [valid, recipe, structure: :::";
        let err = run_intent_recipe(bad_yaml, registry(), true)
            .expect_err("malformed YAML should fail to parse");
        assert!(!err.is_empty());
    }

    #[test]
    fn run_intent_recipe_rejects_missing_required_fields() {
        // Error handling: a syntactically valid YAML document that is missing
        // required recipe fields (e.g. `steps`) should fail cleanly rather than
        // silently no-op.
        let incomplete_yaml = r#"
name: incomplete-recipe
description: missing the steps list entirely
"#;
        assert!(run_intent_recipe(incomplete_yaml, registry(), true).is_err());
    }

    #[test]
    fn bash_step_delegates_to_cli_subprocess_adapter() {
        // Integration: bash steps must actually execute via the runner's
        // CLISubprocessAdapter, not be swallowed by the agent-step resolver.
        // Uses a portable no-op command so the test stays hermetic and fast.
        let a = AzorkAdapter::new(registry());
        let out = a
            .execute_bash_step(
                "echo azork-agent-engine-bash-step",
                ".",
                Some(5),
                &HashMap::new(),
            )
            .expect("bash step should execute successfully");
        assert!(out.contains("azork-agent-engine-bash-step"));
    }
}
