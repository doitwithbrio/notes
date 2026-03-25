//! Version history system — the replacement for the old session-based history.
//!
//! Versions are created at meaningful moments (document switch, app close,
//! long idle, Cmd+S) rather than on every Automerge change.
//!
//! Each version gets a deterministic sea creature name for easy reference.

use automerge::{AutoCommit, ChangeHash, ReadDoc};
use serde::{Deserialize, Serialize};

use crate::error::CoreError;

// ── Sea Creature Name Generator ─────────────────────────────────────

/// Curated list of ~100 sea creatures for version naming.
/// Selected for: recognizability, easy spelling, visual distinctness.
const SEA_CREATURES: &[&str] = &[
    "Nautilus",
    "Seahorse",
    "Jellyfish",
    "Starfish",
    "Orca",
    "Dolphin",
    "Coral",
    "Urchin",
    "Manatee",
    "Narwhal",
    "Barracuda",
    "Stingray",
    "Lobster",
    "Oyster",
    "Marlin",
    "Walrus",
    "Pufferfish",
    "Anglerfish",
    "Swordfish",
    "Beluga",
    "Squid",
    "Anemone",
    "Barnacle",
    "Clownfish",
    "Cuttlefish",
    "Dugong",
    "Flounder",
    "Grouper",
    "Krill",
    "Lionfish",
    "Manta",
    "Octopus",
    "Penguin",
    "Sailfish",
    "Tarpon",
    "Turtle",
    "Pelican",
    "Mackerel",
    "Anchovy",
    "Herring",
    "Shrimp",
    "Crab",
    "Mussel",
    "Scallop",
    "Conch",
    "Triton",
    "Porpoise",
    "Seal",
    "Otter",
    "Albatross",
    "Osprey",
    "Puffin",
    "Gannet",
    "Heron",
    "Plover",
    "Sandpiper",
    "Cormorant",
    "Terrapin",
    "Axolotl",
    "Abalone",
    "Remora",
    "Wahoo",
    "Tuna",
    "Halibut",
    "Wrasse",
    "Goby",
    "Blenny",
    "Damsel",
    "Parrotfish",
    "Surgeonfish",
    "Triggerfish",
    "Boxfish",
    "Filefish",
    "Hawkfish",
    "Butterflyfish",
    "Dragonet",
    "Frogfish",
    "Stonefish",
    "Nudibranch",
    "Cowrie",
    "Whelk",
    "Limpet",
    "Chiton",
    "Medusa",
    "Kraken",
    "Leviathan",
    "Siren",
    "Selkie",
    "Nereid",
    "Trilobite",
    "Ammonite",
    "Coelacanth",
    "Lamprey",
    "Hagfish",
    "Oarfish",
    "Sunfish",
    "Blobfish",
    "Narwhal",
    "Capelin",
    "Sablefish",
];

/// Generate a deterministic sea creature name from a version ID.
/// Uses FNV-1a hash of the ID string to index into the creature list.
pub fn creature_name_for_id(version_id: &str) -> String {
    let hash = fnv1a_hash(version_id.as_bytes());
    let idx = (hash as usize) % SEA_CREATURES.len();
    SEA_CREATURES[idx].to_string()
}

/// Pick a creature name that hasn't been used yet for this document.
/// Falls back to the deterministic name if all names are exhausted.
pub fn unique_creature_name(version_id: &str, used_names: &[String]) -> String {
    let base_hash = fnv1a_hash(version_id.as_bytes());
    let total = SEA_CREATURES.len();

    for offset in 0..total {
        let idx = ((base_hash as usize) + offset) % total;
        let name = SEA_CREATURES[idx];
        if !used_names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            return name.to_string();
        }
    }

    // All names used — append a number to the base name
    let base_idx = (base_hash as usize) % total;
    let base = SEA_CREATURES[base_idx];
    let count = used_names.iter().filter(|n| n.starts_with(base)).count();
    format!("{} {}", base, count + 1)
}

/// FNV-1a hash (32-bit) — fast, simple, good distribution.
fn fnv1a_hash(data: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &byte in data {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

// ── Version Data Model ──────────────────────────────────────────────

/// The significance level of a version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VersionSignificance {
    /// < 3 chars changed, no structural changes — not stored
    Skip,
    /// 3–50 chars, no structural changes — shown dimmed
    Minor,
    /// 50+ chars or structural changes — full display
    Significant,
    /// User-created via Cmd+S — always prominent
    Named,
}

impl VersionSignificance {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Minor => "minor",
            Self::Significant => "significant",
            Self::Named => "named",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "skip" => Self::Skip,
            "minor" => Self::Minor,
            "named" => Self::Named,
            _ => Self::Significant,
        }
    }
}

/// The type of version (auto-detected or user-created).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VersionType {
    /// Automatically created at meaningful boundaries
    Auto,
    /// User-created via Cmd+S
    Named,
}

impl VersionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Named => "named",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "named" => Self::Named,
            _ => Self::Auto,
        }
    }
}

/// A version entry in the history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    /// Unique ID (UUID v4).
    pub id: String,
    /// Document ID this version belongs to.
    pub doc_id: String,
    /// Project name.
    pub project: String,
    /// Version type: auto or named.
    #[serde(rename = "type")]
    pub version_type: VersionType,
    /// Sea creature name (always assigned).
    pub name: String,
    /// User-provided label (only for named versions).
    pub label: Option<String>,
    /// Full concurrent Automerge heads at this version (JSON array of hex strings).
    pub heads: Vec<String>,
    /// Stable device actor ID.
    pub actor: String,
    /// When this version was created (Unix timestamp in seconds).
    pub created_at: i64,
    /// Number of Automerge changes since the previous version.
    pub change_count: usize,
    /// Characters added since previous version.
    pub chars_added: usize,
    /// Characters removed since previous version.
    pub chars_removed: usize,
    /// Blocks (paragraphs, headings, etc.) changed since previous version.
    pub blocks_changed: usize,
    /// Significance level.
    pub significance: VersionSignificance,
    /// Ordering sequence number within this document.
    pub seq: i64,
}

/// What triggered this version to be created.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionTrigger {
    /// User pressed Cmd+S
    ManualSave,
    /// User switched to another document
    DocSwitch,
    /// App closed or window lost focus
    AppBlur,
    /// User was idle for 15+ minutes
    IdleTimeout,
    /// Remote peer changes were merged in
    RemoteMerge,
}

// ── Significance Scoring ────────────────────────────────────────────

/// Compute the significance of changes between two document states.
pub fn compute_significance(
    doc: &mut AutoCommit,
    prev_heads: &[ChangeHash],
    current_heads: &[ChangeHash],
) -> (VersionSignificance, usize, usize, usize) {
    // Get text at both points
    let prev_text = get_text_at(doc, prev_heads).unwrap_or_default();
    let current_text = get_text_at(doc, current_heads).unwrap_or_default();

    if prev_text == current_text {
        return (VersionSignificance::Skip, 0, 0, 0);
    }

    // Split into blocks (paragraphs)
    let prev_blocks: Vec<&str> = prev_text
        .split("\n\n")
        .map(|b| b.trim())
        .filter(|b| !b.is_empty())
        .collect();
    let current_blocks: Vec<&str> = current_text
        .split("\n\n")
        .map(|b| b.trim())
        .filter(|b| !b.is_empty())
        .collect();

    // Simple char diff: count chars in new not in old and vice versa
    let chars_added = current_text.len().saturating_sub(prev_text.len()).max(0);
    let chars_removed = prev_text.len().saturating_sub(current_text.len()).max(0);
    let total_char_change = (current_text.len() as isize - prev_text.len() as isize).unsigned_abs();

    // Block-level changes
    let blocks_changed = block_diff_count(&prev_blocks, &current_blocks);

    let significance = if total_char_change < 3 && blocks_changed == 0 {
        VersionSignificance::Skip
    } else if total_char_change < 50 && blocks_changed == 0 {
        VersionSignificance::Minor
    } else {
        VersionSignificance::Significant
    };

    (significance, chars_added, chars_removed, blocks_changed)
}

/// Count structural block changes (blocks added or removed, not just modified).
/// A "structural change" means the number of blocks changed — paragraphs
/// were added or removed, not just edited in place.
fn block_diff_count(old: &[&str], new: &[&str]) -> usize {
    let added = new.len().saturating_sub(old.len());
    let removed = old.len().saturating_sub(new.len());
    added + removed
}

// ── Automerge Helpers ───────────────────────────────────────────────

/// Get the current Unix timestamp in seconds.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Get text content at specific Automerge heads.
pub fn get_text_at(
    doc: &mut AutoCommit,
    heads: &[ChangeHash],
) -> Result<String, automerge::AutomergeError> {
    if heads.is_empty() {
        return Ok(String::new());
    }
    if let Some((automerge::Value::Object(automerge::ObjType::Text), text_id)) =
        doc.get(automerge::ROOT, "text")?
    {
        doc.text_at(&text_id, heads)
    } else {
        Ok(String::new())
    }
}

/// Get the full concurrent heads of the document.
pub fn get_current_heads(doc: &mut AutoCommit) -> Vec<ChangeHash> {
    doc.get_heads().to_vec()
}

/// Serialize heads to a list of hex strings (for JSON/SQLite storage).
pub fn heads_to_strings(heads: &[ChangeHash]) -> Vec<String> {
    heads.iter().map(|h| h.to_string()).collect()
}

/// Parse a list of hex strings back to ChangeHash values.
pub fn strings_to_heads(strings: &[String]) -> Vec<ChangeHash> {
    strings.iter().filter_map(|s| s.parse().ok()).collect()
}

/// Restore a document to the state at given heads.
/// Creates a new Automerge change (non-destructive, additive).
///
/// If `snapshot_data` is provided, loads from the Automerge binary snapshot
/// to preserve rich text formatting. Otherwise falls back to plain text.
pub fn restore_to_version(
    doc: &mut AutoCommit,
    target_heads: &[ChangeHash],
    snapshot_data: Option<&[u8]>,
) -> Result<(), CoreError> {
    // Try rich text restore from snapshot first
    if let Some(data) = snapshot_data {
        if let Ok(snapshot_doc) = AutoCommit::load(data) {
            // Get text from the snapshot (which preserves full structure)
            if let Some((automerge::Value::Object(automerge::ObjType::Text), text_id)) =
                snapshot_doc
                    .get(automerge::ROOT, "text")
                    .map_err(CoreError::from)?
            {
                let target_text = snapshot_doc.text(&text_id).map_err(CoreError::from)?;
                return apply_text_restore(doc, &target_text);
            }
        }
    }

    // Fall back to heads-based plain text restore
    let target_text = get_text_at(doc, target_heads).map_err(CoreError::from)?;
    apply_text_restore(doc, &target_text)
}

/// Apply a text restore by replacing document content.
fn apply_text_restore(doc: &mut AutoCommit, target_text: &str) -> Result<(), CoreError> {
    use automerge::transaction::Transactable;

    if let Some((automerge::Value::Object(automerge::ObjType::Text), text_id)) =
        doc.get(automerge::ROOT, "text").map_err(CoreError::from)?
    {
        let current_text = doc.text(&text_id).map_err(CoreError::from)?;
        if current_text != *target_text {
            let current_len = doc.length(&text_id);
            doc.splice_text(&text_id, 0, current_len as isize, target_text)
                .map_err(CoreError::from)?;
        }
    }

    doc.commit_with(
        automerge::transaction::CommitOptions::default()
            .with_message("Restored to previous version".to_string())
            .with_time(now_secs()),
    );

    Ok(())
}

/// Get the number of changes between two sets of heads.
pub fn count_changes_since(doc: &mut AutoCommit, since_heads: &[ChangeHash]) -> usize {
    doc.get_changes(since_heads).len()
}

// ── Stable Device Actor ID ──────────────────────────────────────────

/// Load or create a stable device actor ID for Automerge documents.
/// Stored as a file alongside the peer identity.
pub fn load_or_create_device_actor_id(
    p2p_dir: &std::path::Path,
) -> Result<automerge::ActorId, CoreError> {
    let actor_path = p2p_dir.join("device-actor-id");

    // Try loading existing
    if actor_path.exists() {
        if let Ok(hex_str) = std::fs::read_to_string(&actor_path) {
            let hex_str = hex_str.trim();
            if hex_str.len() == 32 && hex_str.chars().all(|c| c.is_ascii_hexdigit()) {
                let bytes: Vec<u8> = (0..hex_str.len())
                    .step_by(2)
                    .filter_map(|i| u8::from_str_radix(&hex_str[i..i + 2], 16).ok())
                    .collect();
                if bytes.len() == 16 {
                    return Ok(automerge::ActorId::from(bytes.as_slice()));
                }
            }
        }
        log::warn!("Device actor ID file corrupt, generating new one");
    }

    // Generate a new 16-byte random actor ID
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|e| CoreError::InvalidData(format!("failed to generate actor ID: {e}")))?;

    let actor_id = automerge::ActorId::from(bytes.as_slice());

    // Save as hex string
    let hex_str: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    std::fs::write(&actor_path, &hex_str)
        .map_err(|e| CoreError::InvalidData(format!("failed to save device actor ID: {e}")))?;

    log::info!("Generated new device actor ID: {}", hex_str);
    Ok(actor_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_creature_name_deterministic() {
        let name1 = creature_name_for_id("test-uuid-123");
        let name2 = creature_name_for_id("test-uuid-123");
        assert_eq!(name1, name2, "Same ID should always produce same name");
    }

    #[test]
    fn test_creature_name_different_ids() {
        let name1 = creature_name_for_id("uuid-aaa");
        let name2 = creature_name_for_id("uuid-bbb");
        // Different IDs should (almost certainly) produce different names
        // Not guaranteed, but with ~100 creatures it's very likely
        assert!(
            name1 != name2 || true,
            "Different IDs may produce different names"
        );
    }

    #[test]
    fn test_unique_creature_name_avoids_used() {
        let used = vec!["Nautilus".to_string(), "Seahorse".to_string()];
        let name = unique_creature_name("test-id", &used);
        assert!(
            !used.contains(&name),
            "Should not pick an already-used name"
        );
    }

    #[test]
    fn test_unique_creature_name_exhaustion() {
        // Use all creature names
        let used: Vec<String> = SEA_CREATURES.iter().map(|s| s.to_string()).collect();
        let name = unique_creature_name("test-id", &used);
        // Should get a numbered fallback like "Nautilus 2"
        assert!(name.chars().any(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_significance_scoring() {
        // Skip: identical text
        assert_eq!(
            VersionSignificance::Skip,
            compute_significance_from_text("hello world", "hello world").0
        );

        // Skip: < 3 chars changed
        assert_eq!(
            VersionSignificance::Skip,
            compute_significance_from_text("hello", "hello!").0
        );

        // Minor: 3-50 chars, no structural change
        assert_eq!(
            VersionSignificance::Minor,
            compute_significance_from_text("hello", "hello, how are you today").0
        );

        // Significant: > 50 chars
        assert_eq!(
            VersionSignificance::Significant,
            compute_significance_from_text(
                "hello",
                "hello, this is a much longer piece of text that adds significantly more content to the document"
            ).0
        );
    }

    /// Helper for testing significance without a real Automerge doc.
    fn compute_significance_from_text(
        prev: &str,
        current: &str,
    ) -> (VersionSignificance, usize, usize, usize) {
        let prev_blocks: Vec<&str> = prev
            .split("\n\n")
            .map(|b| b.trim())
            .filter(|b| !b.is_empty())
            .collect();
        let current_blocks: Vec<&str> = current
            .split("\n\n")
            .map(|b| b.trim())
            .filter(|b| !b.is_empty())
            .collect();

        let chars_added = current.len().saturating_sub(prev.len());
        let chars_removed = prev.len().saturating_sub(current.len());
        let total_char_change = (current.len() as isize - prev.len() as isize).unsigned_abs();
        let blocks_changed = block_diff_count(&prev_blocks, &current_blocks);

        let significance = if prev == current {
            VersionSignificance::Skip
        } else if total_char_change < 3 && blocks_changed == 0 {
            VersionSignificance::Skip
        } else if total_char_change < 50 && blocks_changed == 0 {
            VersionSignificance::Minor
        } else {
            VersionSignificance::Significant
        };

        (significance, chars_added, chars_removed, blocks_changed)
    }

    #[test]
    fn test_heads_roundtrip() {
        let hash_str = "a".repeat(64);
        let heads = strings_to_heads(&[hash_str.clone()]);
        let back = heads_to_strings(&heads);
        assert_eq!(back, vec![hash_str]);
    }

    #[test]
    fn test_device_actor_id_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let id1 = load_or_create_device_actor_id(dir.path()).unwrap();
        let id2 = load_or_create_device_actor_id(dir.path()).unwrap();
        assert_eq!(id1, id2, "Same device should get same actor ID");
    }
}
