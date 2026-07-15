//! Feature-gated `recipe-runner-rs` integration (`agentic`).
//!
//! This is the *live* counterpart to the offline [`MockAdapter`]. It embeds the
//! [`recipe-runner-rs`](https://crates.io/crates/recipe-runner-rs) engine the same
//! way Simard and Powderfinger do: azork implements the runner's [`Adapter`]
//! trait, then hands it to `run_recipe`, letting an amplihack recipe orchestrate
//! multi-step intent resolution (agent steps + bash steps) around azork's own
//! learned [`CapabilityRegistry`].
//!
//! The bridge is deterministic where it can be: an *agent step* resolves its
//! prompt against the learned registry via the offline resolver (no LLM, no
//! network), while *bash steps* are delegated to the runner's standard CLI
//! subprocess adapter so recipes can still shell out to `az` for real work.
//!
//! Because it pulls edition-2024, heavier dependencies, the whole module is
//! gated behind the `agentic` Cargo feature and never links into the default
//! offline build.

use std::collections::HashMap;

use recipe_runner_rs::adapters::cli_subprocess::CLISubprocessAdapter;
use recipe_runner_rs::adapters::Adapter as RecipeAdapter;
use recipe_runner_rs::models::RecipeResult;
use recipe_runner_rs::run_recipe;

use crate::agent::{Adapter, MockAdapter};
use crate::capabilities::CapabilityRegistry;

/// Bridges azork into the `recipe-runner-rs` [`Adapter`] seam.
///
/// * Agent steps → resolve the prompt against learned capabilities using the
///   deterministic offline resolver (so the default agentic path stays
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
        // Resolve the intent against what azork has learned so far. This is the
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

/// Run an inline amplihack recipe with azork as the adapter.
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

/// A minimal built-in recipe that resolves a free-text intent into an azork
/// narration via a single agent step. Callers substitute `{{ intent }}` through
/// the recipe context, but the default embeds a placeholder so it parses
/// stand-alone.
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
}
