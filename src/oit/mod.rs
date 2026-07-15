//! The Outside-In-Testing (OIT) agent library.
//!
//! This module holds the *pure, offline-testable* core of the OIT agent: the
//! [`guardrails`] that make the mission's safety contract enforceable in code,
//! the [`usecases`] catalog and friction-detection heuristics, and the
//! [`report`] renderer. The live orchestration that shells out to `az` and drives
//! the `azork` binary lives in the `azork-oit` binary (`src/bin/azork-oit.rs`),
//! which mirrors the recipe-runner-driven agent architecture of Simard and
//! Powderfinger: a deterministic library core with a thin live driver on top.

pub mod guardrails;
pub mod report;
pub mod usecases;
