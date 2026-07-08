//! Self-updating model registry ("phone home").
//!
//! `models.json` at the repo root is the canonical manifest — it's compiled
//! into the binary via `include_str!` so `lantern models` and offline
//! fallbacks always have something to compare against. At runtime we prefer
//! a 24h-TTL cache at `~/.lantern/data/models_cache.json`, refreshed from
//! `MANIFEST_URL` via `curl` (no `reqwest` dependency in this binary — see
//! `Cargo.toml`). `startwork::patterns`'s menu functions call
//! `load_menu_override()` to prefer the cache when present, falling back to
//! their compiled-in tables otherwise.
//!
//! `lantern up` / `lantern doctor` call `spawn_freshness_check()`, which
//! detaches a plain OS thread to do the fetch-and-compare off the hot path.
//! This is deliberately fire-and-forget: those commands must never block on
//! or fail because of network access, so the warning (if any) is genuinely
//! best-effort and may not print before a very short-lived command exits.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/Palmetto-Interactive-LLC/Lantern/main/models.json";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const FETCH_TIMEOUT_SECS: &str = "5";

/// One entry in the model manifest — mirrors `startwork::patterns::ModelChoice`
/// plus a `tier` classifier ("executor" | "orchestrator").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelEntry {
    pub label: String,
    pub agent: String,
    pub model_id: String,
    pub effort: String,
    pub tier: String,
}

/// The full manifest shape published at `models.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub manifest_version: u32,
    pub updated: String,
    pub frontier: BTreeMap<String, String>,
    pub models: Vec<ModelEntry>,
}

/// The manifest checked into the repo at build time.
pub fn compiled_manifest() -> Manifest {
    serde_json::from_str(include_str!("../models.json"))
        .expect("models.json must parse (checked in, validated by CI)")
}

fn cache_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("home directory required")?;
    Ok(home.join(".lantern").join("data").join("models_cache.json"))
}

/// Read the cached manifest from disk regardless of freshness. This is a
/// plain file read, never a network call — safe to call from interactive
/// menu-resolution paths (`startwork::patterns`).
pub fn load_menu_override() -> Option<Manifest> {
    let path = cache_path().ok()?;
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn cache_is_fresh(path: &PathBuf) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|modified| {
            SystemTime::now()
                .duration_since(modified)
                .map(|age| age < CACHE_TTL)
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

/// Fetch the manifest from GitHub via `curl` (no `reqwest` dependency in this
/// binary; see `Cargo.toml`). Bounded by a 5s timeout. Callers must treat
/// failures as non-fatal — this is meant to be used from a background thread
/// or an explicit `lantern models sync`, never on a path that must succeed
/// offline.
pub fn fetch_remote() -> Result<Manifest> {
    let output = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", FETCH_TIMEOUT_SECS, MANIFEST_URL])
        .output()
        .context("failed to spawn curl")?;
    if !output.status.success() {
        anyhow::bail!(
            "curl exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    serde_json::from_slice(&output.stdout).context("remote models.json failed to parse")
}

fn write_cache(manifest: &Manifest) -> Result<()> {
    let path = cache_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::File::create(&path)?;
    file.write_all(serde_json::to_string_pretty(manifest)?.as_bytes())?;
    Ok(())
}

/// Reuse the cache when it's within TTL; otherwise fetch and persist. Never
/// panics — falls back to the stale cache, then the compiled-in manifest.
fn manifest_fetch_if_stale() -> Manifest {
    if let Ok(path) = cache_path() {
        if cache_is_fresh(&path) {
            if let Some(cached) = load_menu_override() {
                return cached;
            }
        }
    }
    match fetch_remote() {
        Ok(manifest) => {
            let _ = write_cache(&manifest);
            manifest
        }
        Err(_) => load_menu_override().unwrap_or_else(compiled_manifest),
    }
}

/// Model ids present in `remote` but absent from `local`, formatted for a
/// one-line warning. `None` when there's no diff.
pub fn diff_ids(local: &Manifest, remote: &Manifest) -> Option<String> {
    let local_ids: BTreeSet<&str> = local.models.iter().map(|m| m.model_id.as_str()).collect();
    let new_ids: Vec<&str> = remote
        .models
        .iter()
        .map(|m| m.model_id.as_str())
        .filter(|id| !local_ids.contains(id))
        .collect();
    if new_ids.is_empty() {
        None
    } else {
        Some(new_ids.join(", "))
    }
}

/// Non-blocking freshness check for `lantern up` / `lantern doctor`. Detaches
/// a plain OS thread that fetches (or reuses the TTL cache), compares against
/// the compiled-in menu, and prints a one-line warning on stderr if the
/// remote manifest has model ids the local binary doesn't know about. Never
/// blocks or fails the calling command — all errors are swallowed.
pub fn spawn_freshness_check() {
    std::thread::spawn(|| {
        let compiled = compiled_manifest();
        let remote = manifest_fetch_if_stale();
        if let Some(diff) = diff_ids(&compiled, &remote) {
            eprintln!("model manifest has newer entries: {diff} — run `lantern models sync`");
        }
    });
}

/// `lantern models`: print the compiled-in menu vs. the cached manifest
/// without writing anything or touching the network.
pub async fn print_status() -> Result<()> {
    let compiled = compiled_manifest();
    let cached = load_menu_override();

    println!("Compiled-in manifest (updated {}):", compiled.updated);
    for m in &compiled.models {
        println!("  [{}] {} {} ({})", m.tier, m.label, m.model_id, m.effort);
    }
    println!();

    match cached {
        Some(cached) => {
            println!("Cached manifest (updated {}):", cached.updated);
            for m in &cached.models {
                println!("  [{}] {} {} ({})", m.tier, m.label, m.model_id, m.effort);
            }
            match diff_ids(&compiled, &cached) {
                Some(diff) => println!("\nDiff vs. compiled-in: {diff}"),
                None => println!("\nNo diff vs. compiled-in."),
            }
        }
        None => println!("No local cache yet — run `lantern models sync`."),
    }
    Ok(())
}

/// `lantern models sync`: fetch the manifest from GitHub, write it to the
/// cache, and report whether the compiled-in defaults changed.
pub async fn sync() -> Result<()> {
    let compiled = compiled_manifest();
    let remote = tokio::task::spawn_blocking(fetch_remote)
        .await
        .context("sync task panicked")??;
    write_cache(&remote)?;
    println!(
        "Synced models_cache.json (manifest updated {}).",
        remote.updated
    );
    match diff_ids(&compiled, &remote) {
        Some(diff) => println!("Compiled-in defaults are behind: {diff}"),
        None => println!("Compiled-in defaults are up to date."),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiled_manifest_parses_and_matches_menu_contract() {
        let manifest = compiled_manifest();
        assert_eq!(manifest.manifest_version, 1);
        assert!(manifest
            .models
            .iter()
            .any(|m| m.label == "Sonnet 5 High" && m.model_id == "claude-sonnet-5"));
        assert_eq!(
            manifest.frontier.get("claude").map(String::as_str),
            Some("claude-fable-5")
        );
    }

    #[test]
    fn diff_ids_finds_new_remote_entries() {
        let mut local = compiled_manifest();
        let remote = compiled_manifest();
        local.models.retain(|m| m.model_id != "claude-opus-4-8");
        let diff = diff_ids(&local, &remote).expect("expected a diff");
        assert!(diff.contains("claude-opus-4-8"));
    }

    #[test]
    fn diff_ids_none_when_identical() {
        let manifest = compiled_manifest();
        assert!(diff_ids(&manifest, &manifest).is_none());
    }
}
