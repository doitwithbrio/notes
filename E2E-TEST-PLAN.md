# E2E Test Suite — Complete Plan

> Generated from deep codebase analysis by three parallel subagents, each reading all source files sequentially. Every test is grounded in actual implementation details, not just the architecture spec. Confirmed implementation gaps are marked **[GAP]** and represent tests that will fail today — these are fixed using a Red→Green workflow.

---

## Table of Contents

1. [Architecture & Tooling](#1-architecture--tooling)
2. [CI Setup](#2-ci-setup-github-actions)
3. [Workflow: Red → Green](#3-workflow-red--green)
4. [Confirmed Implementation Gaps](#4-confirmed-implementation-gaps)
5. [Suite 01 — Editor](#suite-01--editor)
6. [Suite 02 — CRDT Correctness](#suite-02--crdt-correctness)
7. [Suite 03 — P2P Network / Wire Protocol](#suite-03--p2p-network--wire-protocol)
8. [Suite 04 — Invite Flow (SPAKE2)](#suite-04--invite-flow-spake2)
9. [Suite 05 — Permissions](#suite-05--permissions)
10. [Suite 06 — Encryption](#suite-06--encryption)
11. [Suite 07 — Offline / Reconnect](#suite-07--offline--reconnect)
12. [Suite 08 — Presence Indicators](#suite-08--presence-indicators)
13. [Suite 09 — Version History](#suite-09--version-history)
14. [Suite 10 — Persistence](#suite-10--persistence)
15. [Suite 11 — Search](#suite-11--search)
16. [Suite 12 — Large Document Degradation](#suite-12--large-document-degradation)
17. [Suite 13 — UI / App Shell](#suite-13--ui--app-shell)
18. [Suite 14 — Security / Attack Scenarios](#suite-14--security--attack-scenarios)
19. [Priority Summary Table](#priority-summary-table)

---

## 1. Architecture & Tooling

Two test layers cover the full stack:

```
tests/
├── e2e/                             # Playwright — drives the real Tauri desktop app
│   ├── helpers/
│   │   ├── app.ts                   # launch / teardown a Tauri process
│   │   ├── peer.ts                  # spawn two-app peer setup (Alice + Bob)
│   │   └── fixtures.ts              # shared project/note factories
│   └── suites/
│       ├── 01-editor.spec.ts
│       ├── 02-crdt-sync.spec.ts
│       ├── 03-p2p-network.spec.ts
│       ├── 04-invite.spec.ts
│       ├── 05-permissions.spec.ts
│       ├── 06-offline.spec.ts
│       ├── 07-presence.spec.ts
│       ├── 08-versions.spec.ts
│       ├── 09-large-docs.spec.ts
│       └── 10-ui.spec.ts
│
└── src-tauri/tests/                 # Rust integration tests (cargo test)
    ├── encryption_test.rs           # crypto round-trips, tamper detection, zeroize
    ├── doc_store_test.rs            # concurrency, compaction, corruption recovery
    ├── sync_protocol_test.rs        # wire framing, ACL enforcement, viewer isolation
    ├── invite_test.rs               # SPAKE2, TTL, rate limiting
    ├── permissions_test.rs          # check_role, manifest ACL validation
    ├── persistence_test.rs          # atomic writes, backup recovery, path validation
    ├── version_store_test.rs        # significance thresholds, migration, name uniqueness
    └── security_test.rs             # input validation, attack scenarios
```

### Playwright layer

- Drives the compiled Tauri binary directly (not a browser).
- Multi-peer tests spawn **two separate Tauri processes** with different `--notes-dir` temp directories and different iroh identity keys. They connect via LocalSwarmDiscovery on loopback (no relay needed in CI).
- Each test that touches the filesystem uses an isolated temp directory, cleaned up after the test.

### Rust layer

- Standard `cargo test --workspace` plus `tokio::test` for async cases.
- In-process iroh endpoints for sync protocol tests (two `Endpoint` instances in the same binary — no UI, no Tauri, real networking).
- `tempfile::TempDir` for all filesystem operations.
- `proptest` / `quickcheck` for fuzz-style input validation tests.

---

## 2. CI Setup (GitHub Actions)

```yaml
# .github/workflows/e2e.yml (sketch)
jobs:
  rust-tests:
    runs-on: ubuntu-latest
    steps:
      - cargo test --workspace --all-features

  playwright-tests:
    runs-on: ubuntu-latest
    steps:
      - cargo build --release               # build Tauri binary first
      - npm ci
      - xvfb-run npx playwright test        # headless via virtual display
```

Key CI constraints:
- Linux runner with `xvfb` for headless Tauri/Electron webview.
- Tests are hermetic: no external relay, no internet access needed. iroh LocalSwarmDiscovery works on loopback.
- Playwright test timeout: 60s per test (sync / invite flows can be slow).
- Rust test timeout: 30s per test (compaction, large doc word-count are the slowest).
- Two-process tests: each process gets a unique temp dir via `mktemp -d`; both processes are killed in `afterAll`.

---

## 3. Workflow: Red → Green

Every test is written **first**, run against the current codebase to confirm it fails (or passes), then the gap is fixed. This is the canonical order:

```
1. Write test (expected behavior documented in code)
2. Run: confirm RED (failing) for known gaps, GREEN for working features
3. Fix the implementation
4. Run: confirm GREEN
5. Commit both test and fix together
```

For tests that are expected to pass today (no known gap), they serve as **regression tests** — they go green immediately and must stay green.

Implementation order follows priority: P0 gaps first, then P0 security, then P1 correctness, then P2 edge cases (see [Priority Summary Table](#priority-summary-table)).

---

## 4. Confirmed Implementation Gaps

These are bugs confirmed by reading the source code. Tests for these will fail on the current codebase. They are the first things to fix.

| # | Gap | Location | Impact |
|---|---|---|---|
| G01 | `unsent_changes` in `SyncStatusEvent` is hardcoded to `0` — "N unsent changes" status bar feature never shows real count | `src-tauri/src/lib.rs` | P0 — UX feature broken |
| G02 | Cursor gossip path exists in `PresenceManager` but is not wired to Tauri command — cursors never reach remote peers | `src-tauri/src/presence.rs`, `lib.rs` | P0 — collaboration broken |
| G03 | Frontend ignores `DegradationLevel` returned by backend — no dynamic extension disabling, no warning banner, no feature gating | `src/editor/setup.ts`, components | P0 — safety feature absent |
| G04 | 5-second batch sync for 20k+ word docs not implemented — `sync_trigger` debounce is always 500ms | `src-tauri/src/lib.rs` | P0 — performance feature absent |
| G05 | Todos stored only in Svelte `$state` — lost on every app restart, no backend persistence | `src/state/todos.svelte.ts` | P0 — data loss |
| G06 | `_ownerControlled.peers` role field is not blocked for editors — an editor can modify their own role in the manifest via CRDT merge | `crates/notes-core/src/manifest.rs` | P0 — privilege escalation vector |
| G07 | Sidebar file tree avatar dots not updated from live presence events — `setProjectActivePeers` is only called on project open, not on `updatePresence` | `src/state/documents.svelte.ts` | P1 — stale presence UI |
| G08 | `ConflictHighlight` TipTap extension referenced in architecture doc but not in `package.json` or `editor/setup.ts` | `src/editor/setup.ts` | P1 — feature referenced but missing |
| G09 | Path collision after concurrent file creation is not resolved — two files with the same path can coexist in the manifest after sync | `crates/notes-core/src/manifest.rs` | P1 — manifest inconsistency |
| G10 | `manifest_hex` nibble decode uses `unwrap_or(0)` for invalid hex — silently corrupts the manifest instead of returning an error | `src-tauri/src/lib.rs` (accept_invite) | P1 — data corruption on bad input |
| G11 | `compute_significance`: uses `current.len() - prev.len()` (net diff) — 100 inserts + 99 deletes = 1 net char, so large balanced edits are classified as Skip | `crates/notes-core/src/versions.rs` | P1 — versions not created when expected |
| G12 | `get_project_owner` failure falls back to empty string via `unwrap_or_default()` — all permission checks pass on a shared project whose manifest failed to load | `src-tauri/src/lib.rs` | P1 — permission bypass on error |

---

## Suite 01 — Editor

**Layer:** Playwright (E2E) + Vitest (frontend unit)
**Scope:** TipTap initialization, CRDT change pipeline, editor session lifecycle, content sync, history review mode.

---

### E01 — TipTap initializes with undo/redo disabled

**Why:** The `History` plugin from StarterKit is explicitly disabled (`undoRedo: false`). If it were re-enabled by a dependency update, `Cmd+Z` would bypass Automerge's change-based undo and corrupt collaborative state.

**Setup:** Open a note in the editor.

**Steps:**
1. Type "Hello".
2. Press `Cmd+Z`.

**Expected:** "Hello" is not removed. Undo does nothing. No error in console.

**What could go wrong:** A TipTap or StarterKit version bump silently re-enables History. This test catches that regression.

---

### E02 — Typing produces a CRDT incremental change

**Why:** The entire sync pipeline starts with `updateEditorText` → `Automerge.change` → `pendingChunks`. If this step breaks, no changes reach the backend.

**Setup:** Open a note. Observe `pendingChunks` via IPC event spy.

**Steps:**
1. Type "Hello World".
2. Wait 250ms (flush debounce).
3. Assert that `apply_changes` IPC was called with non-empty `data`.

**Expected:** `apply_changes` called at least once with non-empty bytes.

**What could go wrong:** `updateEditorText` short-circuits if text is unchanged. If the initial doc text happens to equal "Hello World", no change fires — test must use a known-empty document.

---

### E03 — `flushLocalChanges` re-queues data on IPC failure

**Why:** If the backend is temporarily unavailable, pending changes must not be silently dropped. They should stay in `pendingChunks` and be retried on the next flush.

**Setup:** Mock `apply_changes` to fail once, then succeed.

**Steps:**
1. Type text → `flushLocalChanges` fires → IPC fails.
2. Assert `pendingChunks` still has the data.
3. Advance timer → `flushLocalChanges` fires again → IPC succeeds.
4. Assert `pendingChunks` is empty.

**Expected:** Data is never lost; it is retried until IPC succeeds.

---

### E04 — Opening a different doc calls `closeEditorSession` first

**Why:** If session cleanup is skipped, the old document's timers keep firing and pending changes from the old doc can be attributed to the new doc's ID.

**Setup:** Open doc A, make edits. Then open doc B.

**Steps:**
1. Open doc A, type "aaa".
2. Open doc B (different file).
3. Assert that doc A's pending chunks were flushed before doc B loaded.
4. Assert that the editor now shows doc B's content.

**Expected:** Clean session handoff. No timer leaks. Doc A's changes persisted.

---

### E05 — `closeEditorSession` auto-creates a version if significant changes exist

**Why:** Users should not lose version history when switching documents. The auto-version is the safety net.

**Setup:** Open a note, type 60+ characters. Then open another note.

**Steps:**
1. Open doc A, type 60 characters.
2. Open doc B.
3. Call `get_doc_versions(projectId, docAId)`.

**Expected:** At least one version for doc A is returned, with `type: 'auto'`.

**What could go wrong:** If `createVersion` returns `"no significant changes"` (< 50 chars), no version is created. This test uses 60 chars to be safely above the threshold.

---

### E06 — Remote change event updates editor content

**Why:** The core real-time collaboration flow. If the `p2p:remote-change` event doesn't update the editor, Bob's changes are invisible to Alice.

**Setup:** Two Tauri processes (Alice + Bob), both with the same note open.

**Steps:**
1. Bob types "Hello from Bob" in the shared note.
2. Wait for sync.
3. Assert Alice's editor contains "Hello from Bob".

**Expected:** Alice's editor content updates without page reload.

**What could go wrong:** `reloadActiveSession` is async. If the Tauri event fires before the frontend is fully initialized, the event is dropped. The test must wait for the `p2p:remote-change` event to arrive and for the editor DOM to update.

---

### E07 — `applyingRemoteText` flag prevents echo loop

**Why:** When remote content is applied to TipTap, the editor fires an `onUpdate` event. Without the `applyingRemoteText` guard, this would call `updateEditorText` and create a spurious CRDT change that loops back to the peer.

**Setup:** Spy on `apply_changes` IPC calls. Inject a remote change.

**Steps:**
1. Simulate a `p2p:remote-change` event → `reloadActiveSession` → `editor.commands.setContent(...)`.
2. Count `apply_changes` IPC calls in the next 500ms.

**Expected:** Zero `apply_changes` calls triggered by the remote content injection. The guard prevents the echo.

---

### E08 — History review mode: editor non-editable, Cmd+S disabled

**Why:** Users must not accidentally save while reviewing history. The editor must be read-only.

**Setup:** A doc with at least 2 versions.

**Steps:**
1. Open version history panel.
2. Click a past version → enter history review mode.
3. Try to type in the editor.
4. Press `Cmd+S`.
5. Exit history review.

**Expected:**
- Typing does not modify the document.
- `Cmd+S` does not trigger the save prompt.
- `ed.setEditable(false)` was called.
- After exiting review, `ed.setEditable(true)` was called and typing works again.

---

### E09 — Switching docs while in history review exits review mode

**Why:** If review mode persists after navigation, the next document is loaded as non-editable — a hard-to-debug state for the user.

**Setup:** A note in history review mode. A second note in the sidebar.

**Steps:**
1. Enter history review on note A.
2. Click note B in the sidebar.
3. Assert `versionState.isReviewing === false`.
4. Assert the editor is editable.

**Expected:** Review mode is exited. Note B is editable.

---

### E10 — `textToEditorHtml` escapes HTML special characters

**Why:** If user-typed `<`, `>`, `&` are not escaped before being passed to TipTap via `setContent`, they would be interpreted as HTML and could cause rendering errors or XSS in the webview.

**Test (unit):**
```typescript
expect(textToEditorHtml('<script>alert("xss")</script>')).toBe(
  '<p>&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;</p>'
)
expect(textToEditorHtml('A & B')).toBe('<p>A &amp; B</p>')
```

**Expected:** All 5 special chars (`<`, `>`, `&`, `"`, `'`) are escaped.

---

### E11 — `textToEditorHtml` / `editorToPlainText` round-trip

**Why:** The round-trip must be lossless. If a double-newline paragraph split does not survive through HTML and back, documents will gain or lose blank lines on every reload.

**Test (unit):**
```typescript
const original = "Paragraph one.\n\nParagraph two."
const html = textToEditorHtml(original)
const editor = createTestEditor()
editor.commands.setContent(html)
expect(editorToPlainText(editor)).toBe(original)
```

---

### E12 — Editor session timer leak: `clearTimers` called on unmount

**Why:** Four timers (flush, save, idle-version, word-count) run during editing. If they aren't cleared on component destroy, they fire on unmounted components and cause React-style "state update on unmounted component" errors or, worse, IPC calls with a stale `docId`.

**Setup:** Mount the editor component, unmount it, wait 2 seconds.

**Steps:**
1. Open a note (mounts editor).
2. Close the note (unmounts editor).
3. Wait 2000ms.
4. Assert no IPC calls fired after unmount.

---

### E13 — **[GAP]** `ConflictHighlight` extension is not implemented

**Why:** The architecture doc specifies this extension, but it is absent from `package.json` and `editor/setup.ts`. This test documents the gap so it is not forgotten.

**Expected (currently):** The extension does not exist. Test confirms it is absent.
**Expected (after fix):** The extension exists, detects concurrent edits in the same paragraph, and annotates them.

---

## Suite 02 — CRDT Correctness

**Layer:** Rust integration tests + Playwright (two-process)
**Scope:** Automerge convergence, concurrent edits, manifest CRDT, compaction, schema versioning.

---

### C01 — Two-peer text convergence

**Why:** The fundamental correctness property of the system. If two peers type independently and sync, they must reach identical state.

**Setup:** Two Tauri processes (Alice + Bob), same shared project, same note open.

**Steps:**
1. Disconnect network (disable loopback peer discovery).
2. Alice types "Hello ".
3. Bob types "World".
4. Re-enable network. Wait for sync.
5. Read `get_doc_text` on both peers.

**Expected:** Both peers have the same text (order determined by actor ID, but identical between peers). Neither insertion is lost.

---

### C02 — Idempotent re-apply of same incremental bytes

**Why:** If the network retransmits a sync message, the document must not be corrupted by applying the same Automerge changes twice.

**Setup (Rust):**
```rust
let (bytes1, _) = doc.apply_incremental(change_bytes)?;
let (bytes2, new_hashes) = doc.apply_incremental(change_bytes)?;
assert!(new_hashes.is_empty()); // no new changes on second apply
assert_eq!(bytes1, bytes2);     // document state identical
```

**Expected:** Second application produces no new hashes. Document state unchanged.

---

### C03 — Concurrent file creation in manifest converges

**Why:** Alice and Bob can both create files simultaneously when offline. Both must appear after sync — no file silently wins over the other.

**Setup (Rust):** Two `ProjectManager` instances with the same manifest doc ID, different actor IDs.

**Steps:**
1. Alice calls `create_note("project", "notes/a.md")`.
2. Bob calls `create_note("project", "notes/b.md")`.
3. Sync the manifest docs between Alice and Bob.
4. Call `list_files` on both.

**Expected:** Both `notes/a.md` and `notes/b.md` appear on both peers.

---

### C04 — Concurrent rename + edit preserves both operations

**Why:** Alice renaming a file and Bob editing its content are independent operations on different documents (manifest vs. content doc). Both must survive the sync.

**Steps:**
1. Alice renames `uuid-1` from `draft.md` to `final.md` in the manifest.
2. Bob inserts "Important content" into doc `uuid-1`.
3. Sync both the manifest and the content doc.
4. Assert path is `final.md` AND content contains "Important content".

**Expected:** Rename and content edit coexist. UUID-based cross-references remain stable.

---

### C05 — `schemaVersion` field is preserved across sync

**Why:** If `schemaVersion` conflicts after sync (two actors both wrote it), Automerge stores it as a conflict value. Code that reads it as a scalar would get undefined behavior.

**Setup (Rust):**
```rust
let val = doc.get(ROOT, "schemaVersion")?;
assert_eq!(val, Some(ScalarValue::Uint(1)));
// After sync with another peer that also set schemaVersion = 1:
// No conflict — same value from both actors is not a conflict in Automerge.
```

**Expected:** `schemaVersion` is a clean scalar `1`, not a conflict value.

---

### C06 — Tombstone accumulation and compaction

**Why:** 1,000 full-document replace cycles create massive tombstone accumulation. Compaction must reduce the document to a manageable size.

**Setup (Rust):**
```rust
for i in 0..1000 {
    doc.replace_text(doc_id, &format!("Content version {}", i)).await?;
}
let size_before = doc.save().len();
doc.compact(doc_id).await?;
let size_after = doc.save().len();
assert!(size_after < size_before / 10); // at least 10x reduction
```

**Expected:** Post-compaction binary is dramatically smaller. Text content is identical.

---

### C07 — **[GAP]** Concurrent insertions: whole-string-replace causes tombstone behavior

**Why:** The current `updateEditorText` uses `d.text = nextText` (full string assignment), not positional splice operations. This means concurrent edits are represented as "delete everything + insert new string" rather than precise character-level operations. After merge, both peers get the correct final text, but the Automerge document accumulates tombstones for every character ever deleted.

**Test:** Documents the behavior. Alice and Bob both type at position 1 simultaneously. After sync:
- Both get the same converged string (correct).
- The document's internal op count is O(n * edits) rather than O(n) (tombstone cost documented).

**Expected (current behavior):** Convergence works but tombstones accumulate faster than with splice-based edits.
**Expected (after fix):** Investigate using `Automerge.splice` for positional edits to reduce tombstone growth.

---

### C08 — **[GAP]** Path collision after concurrent file creation not resolved

**Why:** The architecture doc claims path collisions produce `-1`, `-2` suffix resolution. The implementation does not do this — two files with the same path can coexist in the manifest after sync.

**Test:**
1. Alice and Bob both offline, both create `notes/readme.md`.
2. Sync.
3. Call `list_files` → assert the behavior.

**Expected (current behavior, FAILING):** Two entries with path `notes/readme.md`. No suffix resolution.
**Expected (after fix):** One is renamed to `notes/readme-1.md` and user is notified.

---

### C09 — Empty doc sync after invite

**Why:** After accepting an invite, Bob has placeholder empty docs. The first sync must populate them with Alice's content.

**Setup:** Two-process test. Alice has content in a note. Bob has just accepted the invite.

**Steps:**
1. Alice has text "Alice's note content" in a doc.
2. Bob accepts invite.
3. Wait for initial sync.
4. Bob calls `get_doc_text(projectId, docId)`.

**Expected:** Bob's doc contains "Alice's note content". No empty doc state persists.

---

### C10 — Manifest ACL write rejection: editor cannot change owner

**Why:** The core security property of the permissions system. If an editor can forge an ACL change, they can grant themselves owner permissions.

**Setup (Rust):**
```rust
// Simulate editor Bob trying to change owner to himself
let tampered_manifest = manifest_with_changed_owner("bob-actor-id");
let result = validate_owner_controlled_changes(
    &doc, &before_heads, &alice_actor_hex
);
assert!(result.is_err()); // must be rejected
```

**Expected:** `CoreError::InvalidInput("unauthorized modification of owner-controlled fields")` is returned.

---

### C11 — **[GAP]** Editor can modify `_ownerControlled.peers` role via CRDT merge

**Why:** The manifest validation only checks if `_ownerControlled.owner` and `keyEpoch` changed. It does NOT check if the `peers` map was modified. An editor can write their own role from `"editor"` to `"owner"` via a CRDT merge.

**Test:** Confirms this gap exists today. Documents it as a failing security test until fixed.

**Expected (current, FAILING):** Editor role change in `peers` map passes `validate_owner_controlled_changes`.
**Expected (after fix):** ANY change to `_ownerControlled` by a non-owner actor is rejected.

---

### C12 — Compaction triggers full re-sync from fresh state

**Why:** After Alice compacts a doc, her `SyncState` is cleared. Bob still has his old `SyncState` referencing pre-compaction change hashes. The next sync must converge despite this state mismatch.

**Setup:** Two-process test. Alice and Bob synced. Alice compacts.

**Steps:**
1. Confirm sync convergence.
2. Alice calls `compact_doc`.
3. Alice makes a new change.
4. Sync with Bob.
5. Assert both peers converge to the same final text.

**Expected:** Full re-convergence after compaction. No stuck sync state.

---

## Suite 03 — P2P Network / Wire Protocol

**Layer:** Rust integration tests (in-process iroh endpoints)
**Scope:** Wire header parsing, framing, connection limits, relay E2E encryption, reconnect.

---

### N01 — Valid wire header parsed correctly

**Setup (Rust):**
```rust
let header = encode_stream_header(MessageType::SyncMessage, doc_id);
assert_eq!(header.len(), 34);
assert_eq!(header[0], PROTOCOL_VERSION); // 0x01
assert_eq!(header[1], MessageType::SyncMessage as u8);
assert_eq!(&header[2..], doc_id.as_bytes());
```

---

### N02 — Wrong protocol version byte → protocol error

**Setup (Rust):**
```rust
let mut header = encode_stream_header(MessageType::SyncMessage, doc_id);
header[0] = 0x02; // wrong version
let result = read_stream_header(&header[..]);
assert!(matches!(result, Err(SyncError::Protocol(_))));
```

---

### N03 — Unknown message type → protocol error, connection continues

**Setup (Rust):**
```rust
let mut header = encode_stream_header(MessageType::SyncMessage, doc_id);
header[1] = 0xFF; // unknown type
let result = read_stream_header(&header[..]);
assert!(matches!(result, Err(SyncError::Protocol(_))));
// The connection loop must `continue` not `break` on this error
```

---

### N04 — Frame length > 16 MB → `message too large` error

**Setup (Rust):**
```rust
let len_bytes = (MAX_MESSAGE_SIZE + 1).to_be_bytes();
let result = read_framed_from_bytes(&[len_bytes, &[]].concat()).await;
assert!(matches!(result, Err(SyncError::Protocol(msg)) if msg.contains("message too large")));
```

---

### N05 — Truncated stream header → IO error, graceful close

**Setup (Rust):** Send only 20 of 34 header bytes. Assert `read_exact` returns an IO error, not a panic.

---

### N06 — Zero-length frame is handled without stall

**Setup (Rust):**
```rust
let frame = encode_framed(&[]);
let result = read_framed(&frame[..]).await;
assert_eq!(result.unwrap(), vec![]);
```

---

### N07 — `MAX_STREAMS_PER_CONNECTION = 16` enforced

**Setup (Rust):** Open 16 concurrent streams on a connection. The 17th `semaphore.acquire()` must block (not proceed immediately).

**Expected:** Throughput limited to 16 concurrent streams. No panic or crash.

---

### N08 — `MAX_CONNECTIONS = 32` enforced

**Setup (Rust):** 32 connections already active (semaphore full). 33rd `try_acquire` returns `Err`. Connection dropped.

**Expected:** The 33rd connection is dropped at the outer level. Existing 32 connections continue normally.

---

### N09 — NodeId spoofing: wrong secret key → handshake failure

**Why:** iroh uses Ed25519 for peer identity. An attacker presenting a fabricated NodeId without the corresponding secret key must fail the QUIC TLS handshake.

**Setup (Rust):** Create two iroh endpoints. Try to connect from endpoint A to endpoint B using endpoint C's NodeId (mismatch).

**Expected:** Connection fails at TLS handshake. No data is exchanged.

---

### N10 — Relay E2E encryption: relay cannot read Automerge content

**Why:** Even if the relay is fully compromised, it must not be able to read note content. All iroh QUIC traffic is E2E encrypted.

**Setup:** Enable packet capture at the relay level. Sync two documents through the relay.

**Expected:** Captured relay packets contain no plaintext Automerge bytes. The relay sees only encrypted QUIC datagrams.

---

### N11 — Reconnect after connection drop fires within 30 seconds

**Setup (two-process):**
1. Alice and Bob connected.
2. Kill Bob's process (simulates network drop).
3. Restart Bob.
4. Wait ≤ 30 seconds.

**Expected:** Alice's `monitoring_loop` detects the dead connection at the next 15s tick. A new connection is established. `p2p:peer-status: Connected` event fires on Alice's frontend.

---

### N12 — Custom relay URL propagated to iroh endpoint

**Setup:** Set `custom_relays: ["https://my-relay.example.com"]` in settings. Restart. Inspect the built `RelayMode`.

**Expected:** The endpoint uses the custom relay URL, not the N0 default.

---

### N13 — Invalid relay URL is skipped without panic

**Setup:** Set `custom_relays: ["not-a-url"]`. Start the app.

**Expected:** Warning logged. App starts normally. No panic. N0 default relay used as fallback.

---

### N14 — `sync_with_peer` times out after 60s for unreachable peer

**Setup (Rust):** Use a `NodeId` that is not reachable (random key, relay unreachable). Call `sync_with_peer`.

**Expected:** After ≤ 60 seconds, returns `InvalidData("connection timed out")`. Does not hang indefinitely.

---

### N15 — `SyncStateStore.state_path` with short peer ID does not panic

**Setup (Rust):**
```rust
let short_peer_id = "abc"; // less than 10 chars
let path = state_path(&short_peer_id, &doc_id);
// Must not panic with index out of bounds
assert!(path.is_ok());
```

**Note:** The current implementation uses `peer_id.to_string()[..10]` which panics on strings shorter than 10 chars. This is a real bug.

---

### N16 — Concurrent sync of 10 documents without deadlock

**Setup (Rust):** Register 10 docs in a `SyncEngine`. Use `tokio::join!` to fire 10 concurrent `sync_doc_with_peer` calls.

**Expected:** All 10 complete. No DashMap deadlock. No RwLock contention (DashMap handles concurrent reads on different docs without locking).

---

### N17 — Oversized `SignatureBatch` (> 1000 sigs) → rejected with warning

**Setup (Rust):** Craft a `SignatureBatchPayload` with 1001 entries. Send via the `SignatureBatch` stream type.

**Expected:** Warning logged: "Rejected oversized signature batch". No signatures stored. Stream closed normally.

---

### N18 — Sync convergence under simulated message delay

**Setup (Rust):** Mock the QUIC send channel to delay every 3rd message by 500ms. Run `sync_loop` to convergence.

**Expected:** Despite jitter, sync converges. `unproductive_rounds` counter never reaches `MAX_UNPRODUCTIVE = 10`.

---

### N19 — **[GAP]** `unsent_changes` in `SyncStatusEvent` is always 0

**Why:** The "N unsent changes" status bar feature is intended to show how many local changes have not yet reached peers. The backend always sends `unsent_changes: 0`, so the display is permanently misleading when offline.

**Test:** Make changes while offline. Assert that `syncState.unsentChanges > 0`.

**Expected (current, FAILING):** Always 0.
**Expected (after fix):** Returns the count of changes pending sync with at least one peer.

---

## Suite 04 — Invite Flow (SPAKE2)

**Layer:** Rust integration tests + Playwright (two-process for full flow)
**Scope:** SPAKE2 correctness, TTL, rate limiting, payload security, role integrity.

---

### I01 — Full invite flow end-to-end

**Setup:** Two-process test (Alice = owner, Bob = invitee).

**Steps:**
1. Alice opens "Share" dialog → invite code generated (6 BIP-39 words + owner peer ID).
2. Bob opens "Join" dialog → enters passphrase + Alice's peer ID.
3. Wait ≤ 30 seconds.
4. Bob's project list shows the shared project with role `editor`.
5. Bob can read Alice's notes.

**Expected:** Invite completes. Project appears on Bob's side with correct content and role.

---

### I02 — Correct passphrase: SPAKE2 derives identical session key on both sides

**Setup (Rust):**
```rust
let passphrase = b"tiger-marble-ocean-violet-canyon-frost";
let (owner_spake, owner_msg) = Spake2::<Ed25519Group>::start_a(passphrase, b"owner", b"invitee");
let (invitee_spake, invitee_msg) = Spake2::<Ed25519Group>::start_b(passphrase, b"owner", b"invitee");
let owner_key = owner_spake.finish(&invitee_msg)?;
let invitee_key = invitee_spake.finish(&owner_msg)?;
assert_eq!(owner_key, invitee_key);
assert_ne!(owner_key, [0u8; 32]); // not all zeros
```

---

### I03 — Wrong passphrase: SPAKE2 may produce key, but payload decryption must fail

**Why:** SPAKE2 can "succeed" (return Ok) with a wrong passphrase but will derive a different key. The payload encrypted with the correct key will fail to decrypt with the wrong key.

**Setup (Rust):**
```rust
let wrong_key = derive_spake2_key(b"wrong-passphrase");
let payload = encrypt_invite_payload(correct_key, &manifest_bytes)?;
let result = decrypt_invite_payload(wrong_key, &payload);
assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
```

---

### I04 — Invite expires after 10 minutes

**Setup (Rust):**
```rust
let invite = PendingInvite::new(passphrase, project_id, role);
// Fake the creation time to be 11 minutes ago
let expired_invite = invite.with_created_at(Instant::now() - Duration::from_secs(660));
assert!(expired_invite.is_expired());
```

---

### I05 — 4th attempt after 3 wrong passphrases → `Exhausted`

**Setup (Rust):**
```rust
let mut invite = PendingInvite::new(passphrase, project_id, role);
invite.increment_attempts(); // 1
invite.increment_attempts(); // 2
invite.increment_attempts(); // 3
assert!(invite.is_exhausted()); // MAX_HANDSHAKE_ATTEMPTS = 3
```

---

### I06 — Invite is one-time use: second attempt after success → not found

**Setup (Rust):**
```rust
pending_invites.add(passphrase.clone(), invite);
pending_invites.complete(&passphrase); // removes it
let result = pending_invites.get(&passphrase);
assert!(result.is_none());
```

---

### I07 — SPAKE2 message > 256 bytes → `InvalidPayload`

**Setup (Rust):** Send a 257-byte SPAKE2 message in the invite stream. Assert the handler returns `InviteError::InvalidPayload` before processing.

---

### I08 — Invite payload > 16 MB → `InvalidData`

**Setup (Rust):** Craft an invite payload with declared length `16 * 1024 * 1024 + 1`. Assert `read_framed` returns `InvalidData("invite payload too large")`.

---

### I09 — `generate_passphrase(6)` produces 6 unique, valid words

**Setup (Rust):**
```rust
let passphrase = generate_passphrase(6);
let words: Vec<&str> = passphrase.split('-').collect();
assert_eq!(words.len(), 6);
for word in &words { assert!(BIP39_WORDLIST.contains(word)); }
// Run 10 times, all different
let passphrases: HashSet<_> = (0..10).map(|_| generate_passphrase(6)).collect();
assert_eq!(passphrases.len(), 10);
```

---

### I10 — `normalizePassphrase` handles spaces, hyphens, and mixed case

**Setup (Rust or unit test):**
```
normalizePassphrase("Tiger Marble Ocean") → "tiger-marble-ocean"
normalizePassphrase("TIGER-MARBLE-OCEAN") → "tiger-marble-ocean"
normalizePassphrase("tiger  marble  ocean") → "tiger-marble-ocean"
normalizePassphrase("   ") → "" (empty, blocked by guard)
```

---

### I11 — Invitee role is set by owner inside encrypted payload (not forgeable)

**Why:** The `role` field in `InvitePayload` is inside the SPAKE2-encrypted blob. An invitee cannot modify it without breaking the auth tag.

**Setup (Rust):** Decrypt the payload, modify `role` from `"editor"` to `"owner"`, re-serialize. Attempt to re-encrypt with the session key. Assert that a fresh SPAKE2 exchange (the invitee doesn't know the key) cannot produce a valid ciphertext with the modified payload.

**Expected:** The invitee cannot forge a valid role escalation. They receive exactly the role the owner sent.

---

### I12 — Re-join an already-joined project → `ProjectAlreadyExists` handled gracefully

**Setup:** Bob has already joined Alice's project. Bob tries to accept the same (or a new) invite.

**Expected:** `create_project` returns `ProjectAlreadyExists`. The handler catches this, skips project creation, and proceeds to load epoch keys. Bob's existing project is unaffected.

---

### I13 — Owner's manifest gains invitee peer ID after acceptance

**Steps:**
1. Alice generates invite.
2. Bob accepts.
3. Alice calls `list_peers("project")`.

**Expected:** Bob's peer ID appears in Alice's manifest `_ownerControlled.peers` with the assigned role.

---

### I14 — Invitee receives epoch key and can decrypt documents

**Steps:**
1. Alice has an encrypted document.
2. Bob accepts invite.
3. Sync.
4. Bob calls `get_doc_text(projectId, docId)`.

**Expected:** Bob successfully decrypts and reads Alice's document. No `DecryptionFailed` error.

---

### I15 — Relay MITM: relay cannot recover the passphrase

**Why:** SPAKE2's security guarantee is that observing the transcript does not help recover the passphrase (requires solving the Diffie-Hellman problem). This test verifies the relay observes only opaque QUIC packets.

**Setup:** Capture all traffic forwarded by the relay during a SPAKE2 handshake.

**Expected:** No plaintext passphrase bytes in captured traffic. The relay sees only encrypted QUIC datagrams. Content of SPAKE2 messages is not readable.

---

### I16 — **[GAP]** Invalid hex in `manifest_hex` silently corrupts

**Why:** `accept_invite` decodes `manifest_hex` using `unwrap_or(0)` for invalid nibbles. A `g` character in the hex string would decode as `0` rather than returning an error.

**Test:** Send `manifest_hex = "deadgg"` (invalid nibbles). Assert this returns an error, not a silently-corrupted manifest.

**Expected (current, FAILING):** Decodes to `[0xde, 0xad, 0x00]` silently.
**Expected (after fix):** Returns `InvalidData("invalid hex in manifest")`.

---

## Suite 05 — Permissions

**Layer:** Rust integration tests + Playwright (two-process for enforcement)
**Scope:** `check_role`, manifest ACL, viewer isolation, ACL initialization.

---

### P01 — Local-only project: all operations allowed

**Setup (Rust):**
```rust
let owner = ""; // local project, no owner set
assert!(check_role(&project, &local_peer_id, MinRole::Owner).is_ok());
assert!(check_role(&project, &local_peer_id, MinRole::Editor).is_ok());
```

---

### P02 — Owner: all operations allowed

**Setup (Rust):** Project has `owner = "alice-peer-id"`. Check as Alice.
```rust
assert!(check_role(&project, &alice_peer_id, MinRole::Owner).is_ok());
assert!(check_role(&project, &alice_peer_id, MinRole::Editor).is_ok());
```

---

### P03 — Editor: document operations allowed, peer management denied

**Setup (Rust):** Project has Bob as `PeerRole::Editor`.
```rust
assert!(check_role(&project, &bob_peer_id, MinRole::Editor).is_ok());
assert!(check_role(&project, &bob_peer_id, MinRole::Owner).is_err());
// Error message: "only the project owner can perform this action"
```

---

### P04 — Viewer: `apply_changes` denied

**Setup (Rust):** Carol is `PeerRole::Viewer`.
```rust
assert!(check_role(&project, &carol_peer_id, MinRole::Editor).is_err());
// Error message: "viewers cannot modify documents"
```

---

### P05 — Unknown peer: all writes denied

**Setup (Rust):** Eve's peer ID is not in the ACL.
```rust
assert!(check_role(&project, &eve_peer_id, MinRole::Editor).is_err());
// Error message: "peer not authorized for this project"
```

---

### P06 — Viewer sync stream: receives snapshot, cannot inject changes

**Setup (Rust):** Carol (Viewer) opens a QUIC sync stream for a document.

**Steps:**
1. Carol sends a valid `SyncMessage` stream header.
2. `SyncEngine` checks ACL → `PeerRole::Viewer`.
3. Assert that `run_responder` is NOT called (no bidirectional sync).
4. Assert that a read-only snapshot is sent to Carol.
5. Assert the document is not modified.

---

### P07 — Unknown peer sync stream: closed without modifying doc

**Setup (Rust):** Eve sends a valid sync stream header. `check_peer_role` returns `PeerRole::Unauthorized`.

**Expected:**
- Stream is closed (`send.finish()`).
- Warning is logged.
- Document is not modified.
- Connection loop continues (does not terminate the connection).

---

### P08 — `populate_doc_acl` adds all manifest peers on doc open

**Setup (Rust):**
```rust
let manifest = manifest_with_peers([("alice", Owner), ("bob", Editor), ("carol", Viewer)]);
populate_doc_acl(&sync_engine, &manifest, doc_id).await?;
assert_eq!(sync_engine.get_role(&alice_id, &doc_id), PeerRole::Owner);
assert_eq!(sync_engine.get_role(&bob_id, &doc_id), PeerRole::Editor);
assert_eq!(sync_engine.get_role(&carol_id, &doc_id), PeerRole::Viewer);
```

---

### P09 — `remove_peer` clears ACL for all project docs

**Setup (Rust):** Bob is an editor on a project with 3 docs. Remove Bob.

**Expected:** `sync_engine.get_role(&bob_id, &doc_id)` returns `PeerRole::Unauthorized` for all 3 docs.

---

### P10 — **[GAP]** Editor can modify `_ownerControlled.peers` role via CRDT merge

**Why:** Only `owner` and `keyEpoch` fields are explicitly checked by `validate_owner_controlled_changes`. An editor can CRDT-merge a change to their own `peers[editorId].role` field without it being rejected.

**Test:** Bob (editor) directly modifies the manifest's `peers.bob.role` to `"owner"`. Sync with Alice. Assert `validate_owner_controlled_changes` rejects this.

**Expected (current, FAILING):** Change passes validation (not checked).
**Expected (after fix):** Rejected with `"unauthorized modification of owner-controlled fields"`.

---

### P11 — `get_project_owner` failure → permission bypass on shared project

**Why:** If `get_project_owner` returns `Err`, `unwrap_or_default()` produces an empty string `""`. An empty owner string triggers the "local project" code path in `check_role`, which allows all operations — even on a legitimately shared project whose manifest temporarily failed to load.

**Test:**
1. Create a shared project with Alice as owner.
2. Simulate `get_project_owner` returning `Err` (e.g., manifest file deleted).
3. Call `check_role(..., MinRole::Owner)` as Bob.
4. Assert this returns an error (not `Ok`).

**Expected (current, FAILING):** Returns `Ok` (permission bypass).
**Expected (after fix):** Returns `Err("failed to load project owner")`.

---

### P12 — Read operations require no role check

**Why:** `list_files`, `get_doc_text`, `search_notes` should be callable by viewers. Test that these commands don't call `check_role` with `MinRole::Editor`.

**Setup:** Carol (Viewer) calls each read command.

**Expected:** All read commands succeed for viewers. No permission error.

---

## Suite 06 — Encryption

**Layer:** Rust integration tests
**Scope:** At-rest encryption, key derivation, HKDF domain separation, epoch ratcheting, zeroize, keychain.

---

### K01 — `encrypt_document` / `decrypt_document` round-trip

**Setup (Rust):**
```rust
for size in [0, 1, 1024, 1024*1024, 16*1024*1024] {
    let data = vec![0xAB_u8; size];
    let encrypted = encrypt_document(&key, &doc_id, epoch, &data)?;
    let (decrypted, epoch_out) = decrypt_document(&key, &doc_id, &encrypted)?;
    assert_eq!(decrypted, data);
    assert_eq!(epoch_out, epoch);
}
```

---

### K02 — Two encryptions of same data produce different ciphertexts

```rust
let c1 = encrypt_document(&key, &doc_id, epoch, &data)?;
let c2 = encrypt_document(&key, &doc_id, epoch, &data)?;
assert_ne!(&c1[4..28], &c2[4..28]); // nonces differ (bytes 4-27)
assert_ne!(c1, c2);
```

---

### K03 — Single-bit flip anywhere in ciphertext → `DecryptionFailed`

```rust
let mut corrupted = encrypted.clone();
corrupted[40] ^= 0x01; // flip one bit in ciphertext body
let result = decrypt_document(&key, &doc_id, &corrupted);
assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
```

---

### K04 — Modified epoch byte in header → wrong derived key → `DecryptionFailed`

```rust
let mut corrupted = encrypted.clone();
corrupted[0] = corrupted[0].wrapping_add(1); // modify epoch byte
let result = decrypt_document(&key, &doc_id, &corrupted);
assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
```

---

### K05 — Cross-document decryption: doc-A ciphertext with doc-B key → fails

```rust
let ciphertext_a = encrypt_document(&key, &doc_id_a, epoch, &data)?;
let result = decrypt_document(&key, &doc_id_b, &ciphertext_a);
assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
```

---

### K06 — `encrypt_snapshot` / `decrypt_snapshot` round-trip

Same structure as K01 but using the snapshot encryption API. Verify domain separation: a snapshot ciphertext cannot be decrypted by `decrypt_document` and vice versa.

---

### K07 — HKDF domain separation: snapshot and document keys are distinct

```rust
let doc_key = derive_document_key(&epoch_key, &doc_id, epoch);
let snap_key = derive_history_key(&epoch_key, &doc_id, epoch);
assert_ne!(doc_key, snap_key);
```

---

### K08 — Plaintext Automerge file (pre-encryption) loads without error

**Why:** Migration path. Users upgrading from a pre-encryption build must have their existing plaintext files readable.

```rust
let plaintext_doc = AutoCommit::new().save();
std::fs::write(&path, &plaintext_doc)?;
// load_doc_encrypted must detect plaintext and load without decryption
let result = load_doc_encrypted(&path, &epoch_key_manager).await?;
assert!(!result.is_empty());
```

---

### K09 — Epoch ratchet produces unique keys

```rust
let mut mgr = EpochKeyManager::new();
let keys: Vec<_> = (0..10).map(|_| { mgr.ratchet(); mgr.current_key() }).collect();
let unique_keys: HashSet<_> = keys.iter().collect();
assert_eq!(unique_keys.len(), 10); // all distinct
```

---

### K10 — Per-peer key wrap/unwrap round-trip

```rust
let epoch_key = EpochKey::generate();
let wrapped = wrap_epoch_key(&epoch_key, &owner_x25519_secret, &peer_x25519_public, epoch)?;
let unwrapped = unwrap_epoch_key(&wrapped, &peer_x25519_secret, &owner_x25519_public, epoch)?;
assert_eq!(epoch_key.as_bytes(), unwrapped.as_bytes());
```

---

### K11 — Removed peer cannot decrypt new epoch documents

**Setup (Rust):**
1. Create project with Alice (owner) + Bob (editor).
2. Encrypt doc with epoch 0 key (Bob has this key).
3. Remove Bob → ratchet to epoch 1.
4. Encrypt new doc with epoch 1 key.
5. Assert Bob's epoch 0 key fails to decrypt the epoch 1 doc.

```rust
let result = decrypt_document(&bob_epoch_0_key, &doc_id, &epoch_1_ciphertext);
assert!(matches!(result, Err(CryptoError::DecryptionFailed)));
```

---

### K12 — Device HKDF key is stable across restarts

```rust
let key1 = derive_device_key(&secret_key);
// Simulate restart: same secret key
let key2 = derive_device_key(&secret_key);
assert_eq!(key1, key2);
// Different secret key → different derived key
let other_key = derive_device_key(&other_secret_key);
assert_ne!(key1, other_key);
```

---

### K13 — Identity key corruption → DB key changes → backup recovery activates

**Setup:**
1. Open app, create notes, close.
2. Corrupt the identity key in the keystore (write wrong-length bytes).
3. Reopen app.

**Expected:**
- Warning logged: "Identity key corrupt, generating new one".
- New identity generated (new NodeId).
- SQLCipher databases unreadable (new derived key).
- `VersionStore` backup recovery path fires: old DB renamed to `.bak.{timestamp}`, fresh DB created.
- App starts without crash.

---

### K14 — Derived doc key is zeroized after use (memory safety)

**Setup (Rust, requires miri or careful pointer test):**
```rust
let doc_key_ptr: *const u8;
{
    let doc_key = derive_document_key(&epoch_key, &doc_id, epoch);
    doc_key_ptr = doc_key.as_ptr();
    // doc_key dropped here, ZeroizeOnDrop fires
}
// Check that the memory location is zeroed
// NOTE: This is a best-effort test; the compiler may optimize away the zero.
// Use #[cfg(test)] hooks in the crypto crate to verify zeroize calls.
```

---

### K15 — HKDF context string mismatch causes decryption failure

**Why:** A typo in either the encrypt or decrypt path causes wrong key derivation and total decryption failure. The HKDF context strings must be byte-for-byte identical between encrypt and decrypt.

**Setup (Rust):** Create a test that calls encryption with one context string and decryption with a slightly modified one (one character difference). Assert `DecryptionFailed`.

---

### K16 — **[GAP]** Snapshot encryption failure falls back to plaintext silently

**Why:** If `encrypt_snapshot` fails (e.g., epoch key not loaded), the version store stores a plaintext snapshot alongside encrypted documents. Mixed encryption state in the version store.

**Test:**
1. Simulate `encrypt_snapshot` failing by clearing the epoch key.
2. Create a named version.
3. Inspect the stored snapshot bytes.

**Expected (current, FAILING):** Plaintext snapshot stored without any warning to the user.
**Expected (after fix):** Returns an error instead of silently storing plaintext.

---

### K17 — Epoch `u32::MAX` ratchet does not collide with epoch 0

```rust
let mut mgr = EpochKeyManager::from_epoch(u32::MAX - 1);
mgr.ratchet(); // epoch u32::MAX
let key_max = mgr.current_key();
mgr.ratchet(); // wraps to 0
let key_wrap = mgr.current_key();
assert_ne!(key_max, mgr.get_key(0)); // no collision with epoch 0
```

---

### K18 — macOS keychain: no `.key` file after successful keychain store (release build)

**Setup:** Release build on macOS. Store a key via `KeyStore::store_key`.

**Expected:** The `.key` file in `keys/` directory does NOT exist after a successful keychain write.

---

### K19 — Legacy keychain migration: key moved from old service, old entry deleted

**Setup:** Create a keychain entry under the legacy service `"com.p2pnotes.app"`. Start the app.

**Expected:**
1. Key loaded from legacy service.
2. Key stored under `"com.doitwithbrio.notes"`.
3. Legacy entry deleted.
4. Next load finds key under new service.

---

### K20 — Key name sanitization prevents path traversal

```rust
let path = key_file_path(&keys_dir, "../../../etc/passwd");
// Must resolve to inside keys_dir only
assert!(path.starts_with(&keys_dir));
assert!(!path.to_str().unwrap().contains(".."));
```

---

## Suite 07 — Offline / Reconnect

**Layer:** Playwright (two-process) + Rust integration tests
**Scope:** Offline edits accumulation, sync recovery, unseen changes, graceful shutdown.

---

### O01 — 50 offline edits sync fully after reconnect

**Setup (two-process):**
1. Alice and Bob connected. Sync to convergence.
2. Disable loopback discovery (simulate network drop).
3. Alice makes 50 distinct text changes.
4. Re-enable discovery. Wait ≤ 30 seconds.
5. Read Bob's doc.

**Expected:** All 50 changes are present on Bob's side. No changes lost. Docs converge.

---

### O02 — `SyncState::LocalOnly` emitted while offline, `Synced` after reconnect

**Setup:** Spy on `p2p:sync-status` events.

**Steps:**
1. Go offline.
2. Make a change. Assert `SyncState::LocalOnly` event fires.
3. Go online. Wait for sync. Assert `SyncState::Synced` fires.

---

### O03 — **[GAP]** "N unsent changes" status bar shows 0 even when offline

**Why:** `SyncStatusEvent.unsent_changes` is hardcoded to `0` in the Rust backend. The intended "N unsent changes" display in the status bar never shows a real count.

**Test:**
1. Go offline.
2. Make 5 changes.
3. Assert `syncState.unsentChanges === 5` in the UI.

**Expected (current, FAILING):** Shows `0`.
**Expected (after fix):** Shows `5`.

---

### O04 — Graceful shutdown: all dirty docs saved before exit

**Setup:**
1. Open 3 docs, type in each (mark dirty).
2. Trigger app exit (send SIGTERM / close window).
3. Reopen the app.

**Expected:** All 3 docs contain the changes made before exit. No data loss.

---

### O05 — Persistent sync state loaded on restart — only new changes re-synced

**Setup (two-process, Rust):**
1. Alice and Bob sync to convergence. Both have `SyncStateStore` entries.
2. Alice restarts.
3. Alice and Bob sync again. Count Automerge sync round trips.

**Expected:** Restart sync requires fewer round trips than initial sync (sync state persisted, resumes from last known position).

---

### O06 — Unseen changes flag set correctly after offline sync

**Steps:**
1. Alice and Bob connected. Both have `notes/api.md` open.
2. Alice closes the note. Bob makes changes. Syncs.
3. Alice reopens the app but does NOT open `notes/api.md`.
4. Call `get_unseen_docs("project")`.

**Expected:** `notes/api.md` is in the unseen list. Sidebar shows unread indicator.
After Alice opens it: `mark_doc_seen` called. Indicator clears.

---

### O07 — Auto-reconnect fires within 30 seconds

**Steps:**
1. Alice and Bob connected.
2. Kill Bob (SIGKILL).
3. Wait 15s (monitoring loop tick).
4. Restart Bob.
5. Wait ≤ 30s.

**Expected:** Alice's monitoring loop detects the dead connection, reconnects to Bob, `p2p:peer-status: Connected` fires.

---

### O08 — Sync status event transitions: `Syncing → Synced → LocalOnly`

**Setup:** Spy on all `p2p:sync-status` events during an offline → online cycle.

**Expected sequence:**
1. (offline) → `LocalOnly`
2. (connection established, sync starting) → `Syncing`
3. (sync complete) → `Synced`

---

### O09 — `sync_trigger` channel capacity: > 256 sends handled gracefully

**Setup (Rust):** Fill the `sync_trigger` channel (capacity 256) then send one more.

**Expected:** The 257th `try_send` returns `Lagged` (or `Full`), is logged as a warning, and the sync loop continues. No data loss (the debounce task drains the channel and syncs all pending docs).

---

## Suite 08 — Presence Indicators

**Layer:** Playwright (two-process) + frontend unit tests
**Scope:** Sync status dots, cursor presence, file tree indicators, gossip.

---

### PR01 — Green dot: peer connected and synced

**Setup:**
1. Share project. Bob connected. Last sync successful.
2. Inspect status bar dot color.

**Expected:** `syncState.connection === 'connected'`. Green dot rendered.

---

### PR02 — Yellow dot: sync in progress

**Setup:**
1. Make a change. Sync starts.
2. Inspect status bar before sync completes.

**Expected:** `syncState.connection === 'slow'`. Yellow dot rendered.

---

### PR03 — Gray dot: no peers / offline

**Setup:** Local project, or shared project with no peers connected.

**Expected:** `syncState.connection === 'local'` or `'offline'`. Gray dot rendered.

---

### PR04 — Per-file sync icon transitions

**Setup:** A shared project. Make a change to a specific note.

**Steps:**
1. Change note A. Before sync: `syncStatus === 'local-only'` → cloud-off icon.
2. Sync starts: `syncStatus === 'syncing'` → spinner icon.
3. Sync completes: `syncStatus === 'synced'` → checkmark icon.

---

### PR05 — **[GAP]** Cursor positions NOT transmitted to remote peers

**Why:** `broadcast_presence` in the Tauri command emits the event only back to the local frontend, not via iroh gossip to remote peers. The gossip wiring is incomplete.

**Test:**
1. Alice moves cursor to position 42.
2. Alice calls `broadcast_presence(project, Some(docId), Some(42), None)`.
3. Assert Bob receives a `p2p:presence-update` event with Alice's cursor position.

**Expected (current, FAILING):** Bob receives no presence event.
**Expected (after fix):** Bob's editor shows Alice's cursor at position 42.

---

### PR06 — **[GAP]** Sidebar file tree dots not updated from live presence

**Why:** `setProjectActivePeers` is called on project open, not on `updatePresence`. The sidebar shows which peers have which file open, but this only updates when the project is (re)opened, not in real-time.

**Test:**
1. Bob opens note A. Alice's sidebar should show Bob's dot next to note A.
2. Bob switches to note B. Alice's sidebar should update.

**Expected (current, FAILING):** Sidebar does not update live.
**Expected (after fix):** Sidebar updates within one presence gossip cycle.

---

### PR07 — Stale cursor: peer disconnects, cursor eventually removed

**Setup:** Bob's cursor is visible. Bob disconnects.

**Expected:**
1. `p2p:peer-status: Disconnected` fires.
2. Bob's cursor fades from Alice's editor within 3 seconds (per architecture spec).
3. `presenceState.cursors` entry for Bob is removed.

**Note:** There is no cleanup timer in `presence.svelte.ts` currently. This test will reveal the gap if unfixed.

---

### PR08 — Cursor color determinism and stability

**Setup:** Connect Bob, disconnect, reconnect.

**Expected:** Bob's cursor color is the same both times. Color is determined by hash of `peerId`, not by connection order.

---

### PR09 — More than 5 peers: status bar shows "+N", not overflow

**Setup:** 6 peers connected. All have the project open.

**Expected:** Status bar shows 5 avatars + "+1". Not 6 avatars. No layout overflow.

---

### PR10 — Presence rate limiter: > 10 updates/sec dropped

**Setup (Rust):** Send 20 presence updates in 100ms from one peer.

**Expected:** Only the first 10 are processed (`MIN_UPDATE_INTERVAL_MS = 100ms`). The other 10 are dropped with a debug log. No crash.

---

### PR11 — Oversized presence message dropped silently

**Setup (Rust):** Craft a presence gossip message > 1024 bytes.

**Expected:** Dropped by `process_incoming`. Warning logged. No crash or panic.

---

## Suite 09 — Version History

**Layer:** Rust integration tests + Playwright
**Scope:** Auto-versioning, named versions, significance thresholds, restore, navigation, migration.

---

### V01 — Auto-version created on doc switch (≥ 50 chars changed)

**Steps:**
1. Open note. Type 60 characters.
2. Open a different note (triggers `closeEditorSession`).
3. Call `get_doc_versions(projectId, docId)`.

**Expected:** At least one version with `type: 'auto'` and `significance: 'significant'`.

---

### V02 — Named version via Cmd+S with custom label

**Steps:**
1. Type some text.
2. Press `Cmd+S`.
3. Enter label "Final draft". Confirm.
4. Call `get_doc_versions`.

**Expected:** One version with `type: 'named'`, `label: "Final draft"`.

---

### V03 — Version list ordered newest first

**Setup (Rust):**
```rust
create_version(doc_id, None, "auto")?; // seq=1
create_version(doc_id, None, "auto")?; // seq=2
create_version(doc_id, None, "auto")?; // seq=3
let versions = get_versions(doc_id)?;
assert_eq!(versions[0].seq, 3);
assert_eq!(versions[2].seq, 1);
```

---

### V04 — Restore to previous version: non-destructive, correct content

**Setup:** Doc with content at V1 ("hello"), V2 ("hello world"), V3 ("hello world goodbye").

**Steps:**
1. Restore to V1.
2. Call `get_doc_text`.

**Expected:** Doc contains "hello". V1, V2, V3 still present in history (restore creates V4, doesn't delete). `get_doc_versions` returns 4 entries.

---

### V05 — Significance thresholds at all boundaries

**Setup (Rust):**
```rust
assert_eq!(significance(2, 0), VersionSignificance::Skip);   // < 3
assert_eq!(significance(3, 0), VersionSignificance::Minor);  // >= 3
assert_eq!(significance(50, 0), VersionSignificance::Minor); // 50 < 51
assert_eq!(significance(51, 0), VersionSignificance::Significant); // >= 51
assert_eq!(significance(1, 1), VersionSignificance::Significant);  // structural change
```

---

### V06 — Named version is always `Significant` regardless of char count

**Setup (Rust):** Call `create_version` with `is_named = true` after only 1 character changed.

**Expected:** `VersionSignificance::Named`. Not `Skip`.

---

### V07 — Block-level diff correctness

**Setup (Rust):**
```rust
let diffs = compute_block_diff("", "New content");
assert!(diffs.iter().all(|d| d.kind == DiffKind::Added));

let diffs = compute_block_diff("Old content", "");
assert!(diffs.iter().all(|d| d.kind == DiffKind::Removed));

let diffs = compute_block_diff("same", "same");
assert!(diffs.iter().all(|d| d.kind == DiffKind::Unchanged));
```

---

### V08 — Prev/next version navigation (most-recent-first ordering)

**Setup:** 5 versions. Currently showing V3 (index 2 in newest-first list).

**Steps:**
1. Click "prev" (older) → shows V4 (index 3). `selectedIdx === 3`.
2. Click "next" (newer) → shows V3 again. `selectedIdx === 2`.

**Expected:** Navigation is consistent with most-recent-first ordering.

---

### V09 — `selectNextVersion` at index 0 exits history review

**Setup:** Viewing the newest version (index 0).

**Steps:** Click "next" (newer than newest).

**Expected:** Review mode exited. Live document shown.

---

### V10 — 98+ versions produce unique names (no duplicate sea creature)

**Setup (Rust):**
```rust
let mut used = HashSet::new();
for _ in 0..100 {
    let name = unique_creature_name(&doc_id, &used)?;
    assert!(used.insert(name.clone())); // must not already exist
    used.insert(name);
}
```

**Note:** This also validates that `SEA_CREATURES` has no duplicate entries (a known potential bug).

---

### V11 — Legacy `history.db` migration is idempotent

**Setup (Rust):**
1. Create `history.db` with 5 sessions.
2. Call `migrate_from_legacy_history_db` twice.
3. Call `get_versions`.

**Expected:** Exactly 5 versions, not 10.

---

### V12 — Version store corruption triggers backup recovery

**Setup:**
1. Open app, create versions, close.
2. Corrupt `versions.db` (write garbage bytes).
3. Reopen app.

**Expected:**
- `versions.db` renamed to `versions.db.bak.{timestamp}`.
- WAL and SHM files also renamed.
- Fresh `versions.db` created.
- App starts without crash. Old history gone (accepted — backup available manually).

---

### V13 — **[GAP]** `compute_significance` undercounts balanced edits

**Why:** Significance uses `current.len() - prev.len()` (net character count). 100 inserts + 99 deletes = net +1, which is classified as `Skip` even though the user made substantial changes.

**Test:**
```rust
let prev = "a".repeat(1000);
let curr = "b".repeat(1000); // replaced 1000 chars with 1000 different chars
let sig = compute_significance(&prev, &curr);
assert_ne!(sig, VersionSignificance::Skip); // should be Significant
```

**Expected (current, FAILING):** `Skip` (net diff = 0).
**Expected (after fix):** `Significant` (total changed = 2000 chars).

---

### V14 — Snapshot encryption fallback for pre-encryption files

**Setup (Rust):**
1. Store a plaintext (unencrypted) snapshot in `VersionStore`.
2. Call `get_version_text`.

**Expected:** Text is returned correctly. `decrypt_snapshot` fails on plaintext, but the fallback path (`AutoCommit::load(raw)`) succeeds.

---

### V15 — Stale preview request cancelled on doc switch

**Setup:**
1. Start loading a version preview for doc A (slow, 500ms).
2. Switch to doc B before preview resolves.

**Expected:** The preview for doc A is discarded (request token mismatch). Doc B's content is shown. No stale text from doc A appears.

---

## Suite 10 — Persistence

**Layer:** Rust integration tests
**Scope:** Atomic writes, backup recovery, permissions, path validation.

---

### D01 — No `.tmp` file remains after successful `atomic_write`

```rust
atomic_write(&path, &data).await?;
assert!(!path.with_extension("automerge.tmp").exists());
assert!(path.exists());
```

---

### D02 — Final file has `0o600` permissions on Unix

```rust
atomic_write(&path, &data).await?;
let mode = fs::metadata(&path)?.permissions().mode();
assert_eq!(mode & 0o777, 0o600);
```

---

### D03 — Corrupted primary → `load_doc` falls back to backup

```rust
// Write valid doc, then corrupt the primary
fs::write(&primary, b"garbage")?;
// load_doc must succeed using .bak
let doc = load_doc(&primary).await?;
assert_eq!(doc.get_text()?, expected_text);
```

---

### D04 — Both primary and backup corrupted → descriptive error, no panic

```rust
fs::write(&primary, b"garbage")?;
fs::write(&backup, b"garbage")?;
let result = load_doc(&primary).await;
assert!(result.is_err());
assert!(result.unwrap_err().to_string().contains("Primary and backup both corrupted"));
```

---

### D05 — Encrypted save/load produces identical content

```rust
save_doc_encrypted(&path, &doc, &epoch_key_manager).await?;
let (loaded_bytes, _epoch) = load_doc_encrypted(&path, &epoch_key_manager).await?;
assert_eq!(doc.save(), loaded_bytes);
```

---

### D06 — Path traversal blocked in all filesystem operations

```rust
assert!(validate_relative_path("../evil.md").is_err());
assert!(validate_relative_path("/absolute/path.md").is_err());
assert!(validate_relative_path("a/b/c/d/e/f/g/h/i/j/k.md").is_err()); // > 10 levels
assert!(validate_relative_path("notes/valid.md").is_ok());
```

---

### D07 — Project name validation rejects dangerous inputs

```rust
assert!(validate_project_name("").is_err());           // empty
assert!(validate_project_name(".hidden").is_err());     // starts with dot
assert!(validate_project_name("../escape").is_err());  // traversal
assert!(validate_project_name("CON").is_err());         // Windows reserved
assert!(validate_project_name("NUL").is_err());
assert!(validate_project_name("foo\0bar").is_err());   // null byte
assert!(validate_project_name("日記").is_ok());          // valid Unicode
```

---

### D08 — `export_markdown` creates read-only file (`0o444`)

```rust
export_markdown(&project, &doc_id, text).await?;
let mode = fs::metadata(&exported_path)?.permissions().mode();
assert_eq!(mode & 0o777, 0o444);
```

---

### D09 — `list_projects` skips `.p2p` and hidden directories

```rust
// Create visible project + hidden dir
fs::create_dir(&notes_dir.join("my-project"))?;
fs::create_dir(&notes_dir.join(".hidden"))?;
let projects = list_projects(&notes_dir).await?;
assert_eq!(projects.len(), 1);
assert_eq!(projects[0].name, "my-project");
```

---

### D10 — `delete_key` zeroes file bytes before removal

```rust
let key_data = vec![0xAB_u8; 32];
store.store_key("test-key", &key_data)?;
let path = store.key_file_path("test-key");
store.delete_key("test-key")?;
// File should not exist
assert!(!path.exists());
// If we could inspect the bytes before deletion (via intercepted write), they should be zeros
```

---

### D11 — Concurrent `atomic_write` to same path: last write wins, no partial file

```rust
let writes = (0..10).map(|i| {
    atomic_write(&path, &vec![i as u8; 1024])
});
join_all(writes).await;
// File must exist and contain complete data (not partial from two concurrent writes)
let content = fs::read(&path)?;
assert_eq!(content.len(), 1024);
```

---

### D12 — `DocStore.compact` write-lock duration under load

**Setup (Rust):** Start 10 concurrent read tasks. Trigger compaction. Measure how long reads are blocked.

**Expected:** Compaction completes within 5 seconds for a 1 MB document. Reads are unblocked after compaction finishes.

---

## Suite 11 — Search

**Layer:** Rust integration tests
**Scope:** FTS5 correctness, encryption, index freshness, recovery.

---

### S01 — `search_notes("query")` returns matching documents

```rust
index_doc(&search_index, "project", "doc1", "notes/api.md", "API endpoints guide")?;
index_doc(&search_index, "project", "doc2", "notes/ui.md", "User interface design")?;
let results = search_notes(&search_index, "API", 20)?;
assert_eq!(results.len(), 1);
assert_eq!(results[0].path, "notes/api.md");
```

---

### S02 — `search_project_notes` filters by project

```rust
index_doc(&index, "project-A", "d1", "a.md", "content")?;
index_doc(&index, "project-B", "d2", "b.md", "content")?;
let results = search_project_notes(&index, "project-A", "content", 20)?;
assert_eq!(results.len(), 1); // only project-A result
```

---

### S03 — Unicode query (CJK characters) handled correctly

```rust
index_doc(&index, "proj", "d1", "note.md", "日本語のノート")?;
let results = search_notes(&index, "日本語", 20)?;
assert_eq!(results.len(), 1);
```

---

### S04 — Corrupt search DB → backup recovery → fresh index created

```rust
fs::write(&index_path, b"garbage")?;
let index = SearchIndex::open_with_recovery(&index_path, &device_key)?;
// Must succeed with empty index
let results = search_notes(&index, "anything", 20)?;
assert!(results.is_empty());
```

---

### S05 — Search freshness: doc saved after last reindex is not searchable yet

```rust
// Index doc at T=0
index_doc(&index, "proj", "d1", "note.md", "old content")?;
// Update doc content without re-indexing
update_doc_without_index("new content")?;
// Search for new content
let results = search_notes(&index, "new content", 20)?;
assert!(results.is_empty()); // stale — not yet indexed
// After explicit reindex:
reindex_doc(&index, "proj", "d1", "note.md", "new content")?;
let results = search_notes(&index, "new content", 20)?;
assert_eq!(results.len(), 1);
```

---

### S06 — Search DB unreadable after identity key rotation

**Setup:** Open app, search works. Identity key rotated (corruption simulated). Reopen.

**Expected:** SQLCipher DB cannot be decrypted. `SearchIndex::open_with_recovery` creates a fresh (empty) index. Search returns no results until reindex completes.

---

## Suite 12 — Large Document Degradation

**Layer:** Rust (backend) + Playwright (frontend integration)
**Scope:** Degradation thresholds, frontend behavior at each level.

---

### L01 — 9,999 words → `DegradationLevel::Normal`

```rust
let text = words(9999);
set_doc_text(&doc, &text).await?;
assert_eq!(get_doc_degradation(&project, &doc_id).await?, DegradationLevel::Normal);
```

---

### L02 — 10,000 words → `DegradationLevel::Warning` (boundary inclusive)

```rust
let text_9999 = words(9999);
let text_10000 = words(10000);
assert_eq!(get_doc_degradation_for_text(&text_9999), DegradationLevel::Normal);
assert_eq!(get_doc_degradation_for_text(&text_10000), DegradationLevel::Warning);
```

---

### L03 — 15,001 words → `DegradationLevel::ReducedFeatures`

```rust
assert_eq!(get_doc_degradation_for_text(&words(15001)), DegradationLevel::ReducedFeatures);
```

---

### L04 — 20,001 words → `DegradationLevel::BatchSync`

```rust
assert_eq!(get_doc_degradation_for_text(&words(20001)), DegradationLevel::BatchSync);
```

---

### L05 — 25,001 words → `DegradationLevel::PerformanceMode`

```rust
assert_eq!(get_doc_degradation_for_text(&words(25001)), DegradationLevel::PerformanceMode);
```

---

### L06 — Custom threshold from settings applied correctly

```rust
let settings = AppSettings { large_doc_warning_words: 5000, ..Default::default() };
assert_eq!(get_degradation(&text_4999, &settings), DegradationLevel::Normal);
assert_eq!(get_degradation(&text_5000, &settings), DegradationLevel::Warning);
```

---

### L07 — **[GAP]** Frontend ignores `DegradationLevel` — no dynamic extension disabling

**Why:** The backend returns the correct level, but the frontend's TipTap editor is configured once at initialization with a static extension list. There is no code that disables `CollaborationCursor` at 15k words, or shows the warning banner at 10k words.

**Test (Playwright):**
1. Open a doc. Verify all extensions active.
2. Backend returns `DegradationLevel::Warning`.
3. Assert warning banner is visible in the editor.

**Expected (current, FAILING):** No banner shown, extensions remain enabled.
**Expected (after fix):** Banner shown, cursor decorations disabled at ReducedFeatures.

---

### L08 — **[GAP]** 5s batch sync for 20k+ word docs not implemented

**Why:** Architecture spec says sync interval should increase to 5 seconds for docs at `BatchSync` level. The backend debounce is always 500ms regardless.

**Test:** Open a 20k+ word doc. Make a change. Measure time until sync fires.

**Expected (current, FAILING):** Sync fires in ~500ms (not 5s).
**Expected (after fix):** Sync fires in ~5000ms for `BatchSync` level docs.

---

### L09 — Degradation level not auto-checked on keystroke

**Why:** `get_doc_degradation` is never called automatically after edits. The check must be explicitly triggered from the editor component, which it currently isn't.

**Test (Playwright):** Add words until crossing the 10k threshold. Assert warning banner appears automatically.

**Expected (current behavior):** Banner never appears automatically (no auto-check on edit).
**Expected (after fix):** Banner appears after word count crosses threshold.

---

## Suite 13 — UI / App Shell

**Layer:** Playwright
**Scope:** App boot, theme, settings, quick-open, drag-and-drop, update system.

---

### U01 — App boots: `booting` → `ready` after project list loads

**Steps:**
1. Launch app.
2. Assert loading indicator visible.
3. Wait for `ready` state.
4. Assert project sidebar visible, no loading indicator.

---

### U02 — All 4 Tauri event listeners registered before `ready = true`

**Setup:** Add a pre-`ready` Tauri event spy.

**Expected:** `p2p:remote-change`, `p2p:sync-status`, `p2p:peer-status`, `p2p:presence-update` all registered before the app transitions to `ready`.

---

### U03 — Teardown: all 4 event listeners unregistered on app exit

**Setup:** Spy on `event.unlisten` calls.

**Expected:** All 4 unlisten functions called during teardown. No leaked listeners.

---

### U04 — Theme bootstrap: correct theme before first paint

**Steps:**
1. Set `localStorage.p2p-notes-theme-bootstrap` to a known theme.
2. Reload the webview.
3. Assert the correct CSS custom properties are set on `document.documentElement` before any scripts run.

---

### U05 — Theme bootstrap with stale version falls back to defaults

**Steps:**
1. Set `localStorage.p2p-notes-theme-bootstrap` to `{ v: 99, mode: "dark", accent: "unknown" }`.
2. Reload.
3. Assert defaults applied: `mode = 'system'`, `accent = 'amber'`.

---

### U06 — System dark mode preference is respected

**Steps:**
1. Set theme `mode = 'system'`.
2. Emulate `prefers-color-scheme: dark` in the webview.
3. Assert `data-theme = 'dark'` applied to `document.documentElement`.

---

### U07 — All 4 accent colors produce correct CSS custom properties

**For each accent in `['amber', 'slate', 'clay', 'olive']`:**
1. Set the accent.
2. Assert `--accent-fg`, `--accent-bg`, and `--accent-border` are non-empty strings.
3. Assert the values differ between accents.

---

### U08 — `Cmd+F` opens quick-open; `Escape` closes it

**Steps:**
1. Press `Cmd+F`. Assert quick-open overlay visible.
2. Press `Escape`. Assert overlay hidden.

---

### U09 — Quick-open search is case-insensitive and resets on query change

**Steps:**
1. Create notes "Meeting Notes" and "Daily standup".
2. Open quick-open. Type "meeting".
3. Assert "Meeting Notes" in results. Assert `selectedIndex === 0`.
4. Clear query. Type "st".
5. Assert `selectedIndex === 0` (reset on query change).
6. Assert "Daily standup" visible.

---

### U10 — Context menu clamped to viewport

**Steps:**
1. Right-click on an element near the bottom-right corner of the window.
2. Assert the context menu is fully within the viewport bounds.

---

### U11 — Settings changes debounced (400ms), saved on destroy

**Steps:**
1. Change the display name setting 3 times within 400ms.
2. Assert `save_settings` IPC called only once (debounced).
3. Change a setting. Immediately close settings pane.
4. Assert `save_settings` IPC called synchronously (on-destroy save).

---

### U12 — Two-step peer removal confirmation

**Steps:**
1. Alice opens Peers section. Clicks "remove" next to Bob.
2. Assert "remove?" / "no" confirmation appears. Peer NOT yet removed.
3. Click "no". Assert confirmation dismissed, peer still present.
4. Click "remove" again. Click "remove?" to confirm.
5. Assert Bob removed from peer list. `remove_peer` IPC called.

---

### U13 — **[GAP]** Todos lost on app restart

**Why:** Todos are stored in Svelte `$state` only. There is no backend persistence. They are lost on every restart.

**Test:**
1. Create 3 todos.
2. Restart the app.
3. Assert todos panel is empty.

**Expected (current, confirming the gap):** Todos gone.
**Expected (after fix):** Todos persisted to backend, restored on restart.

---

### U14 — Update system lifecycle

**Steps:**
1. Mock `check_for_update` to return an available update.
2. Assert banner appears with "Update available".
3. Click install.
4. Assert progress bar appears during download.
5. Mock `Finished` event.
6. Assert "Installing..." state shown.
7. After 1500ms: `relaunch()` called.

---

### U15 — Drag-and-drop reorder: no click fired after drag

**Steps:**
1. Drag note A past the threshold (> 5px).
2. Release.
3. Assert no click event fired on the dragged item.
4. Assert `onReorder` called with correct from/to indices.
5. Assert ghost element removed from DOM.

---

## Suite 14 — Security / Attack Scenarios

**Layer:** Rust integration tests
**Scope:** Input validation, attack vectors, error message sanitization, DoS resistance.

---

### SEC01 — `../` in project name rejected before filesystem operation

```rust
let result = create_project("../escape", &notes_dir).await;
assert!(result.is_err());
// No directory created at notes_dir/../escape
assert!(!notes_dir.parent().unwrap().join("escape").exists());
```

---

### SEC02 — Windows reserved names rejected

```rust
for name in ["CON", "NUL", "PRN", "AUX", "COM1", "LPT1", "con", "nul"] {
    assert!(validate_project_name(name).is_err(), "Should reject: {}", name);
}
```

---

### SEC03 — Control characters and null bytes rejected

```rust
assert!(validate_project_name("foo\0bar").is_err());
assert!(validate_project_name("foo\nbar").is_err());
assert!(validate_project_name("foo\tbar").is_err());
```

---

### SEC04 — `apply_changes` > 16 MB rejected at DocStore layer

```rust
let huge_data = vec![0u8; 16 * 1024 * 1024 + 1];
let result = doc_store.apply_incremental_and_collect(&doc_id, &huge_data).await;
assert!(result.is_err());
assert!(result.unwrap_err().to_string().contains("too large"));
```

---

### SEC05 — Editor cannot modify manifest `_ownerControlled.owner` via sync

See C11 (duplicate reference for security context — this is the most important permission test).

---

### SEC06 — Brute-force invite: 3 wrong → exhausted, 4th blocked

See I05.

---

### SEC07 — Invite replay after success → `NotFound`

See I06.

---

### SEC08 — Role in invite payload is owner-encrypted, invitee cannot forge

See I11.

---

### SEC09 — Protocol version downgrade: peer sends v0 → rejected

```rust
let mut header = valid_header();
header[0] = 0x00; // v0
let result = handle_stream_header(&header);
assert!(matches!(result, Err(SyncError::Protocol(_))));
```

---

### SEC10 — Oversized frame allocation: receiver does not pre-allocate 16 MB

**Why:** A malicious peer sends a frame with declared size 16 MB - 1 byte. The receiver allocates `vec![0u8; len]` before any data arrives. Under many concurrent connections, this could exhaust memory.

**Test:** Measure peak memory usage when receiving a max-size frame declaration before data arrives. Assert allocation is bounded or deferred until data arrives.

---

### SEC11 — Blob store: empty hash does not match first file

```rust
let store = BlobStore::new(&blob_dir);
store.import("real-hash-64chars", &data)?;
assert!(!store.has(""));           // empty string must not match
assert!(store.has("real-hash-64chars")); // correct hash matches
```

---

### SEC12 — Blob store: `read(hash)` verifies content matches hash

**Why:** `import` does not verify hash matches content. A hash collision (or adversarial import) could return wrong data. Content should be verified on read.

```rust
store.import(&hash, &data)?;
// Corrupt the stored file
let path = store.path_for(&hash);
fs::write(path, b"corrupted")?;
let result = store.read(&hash);
assert!(result.is_err()); // hash mismatch detected
```

**Note:** This test will currently pass with wrong data (no verification on read). This is a gap.

---

### SEC13 — Error messages do not leak sensitive data to frontend

**Setup:** Trigger various backend errors that involve file paths, peer IDs, epoch keys.

**Expected:** All error messages returned to the frontend via Tauri IPC contain only generic, user-facing text. No file system paths, no hex-encoded keys, no peer IDs in `CoreError::InvalidInput` or `CoreError::InvalidData` responses.

---

### SEC14 — Broadcast channel overflow handled without panic

```rust
let (tx, mut rx) = broadcast::channel::<Uuid>(256);
// Fill channel
for _ in 0..256 { tx.send(Uuid::new_v4()).unwrap(); }
// One more — receiver is lagged
match rx.recv().await {
    Err(broadcast::error::RecvError::Lagged(n)) => { /* log and continue */ }
    _ => panic!("Expected Lagged error"),
}
```

---

### SEC15 — Unauthorized peer cannot drain `stream_semaphore` via repeated headers

**Setup (Rust):** Eve sends 20 stream headers for docs she is not authorized to access. Each header consumes a `stream_semaphore` permit and is released after rejection.

**Expected:** After each rejected stream, the permit is returned. Eve cannot hold all 16 permits by sending 16 unauthorized headers simultaneously, starving legitimate streams from other peers.

---

## Priority Summary Table

| Priority | Suite | Test IDs | Reason |
|---|---|---|---|
| **P0** | Encryption | K01–K12 | Data confidentiality at rest |
| **P0** | Invite | I01–I06, I11, I13, I14 | Auth correctness |
| **P0** | Permissions | P01–P07 | Core permission boundary |
| **P0** | CRDT | C01, C09, C11 | Convergence, security |
| **P0** | Offline | O01–O04 | Data integrity |
| **P0** | Security | SEC01–SEC09 | Attack vectors |
| **P0** | **[GAP]** | G01, G02, G03, G04, G05, G06 | Confirmed broken features |
| **P1** | Editor | E01–E12 | Core editing pipeline |
| **P1** | Wire protocol | N01–N08, N11, N14, N16 | Protocol correctness |
| **P1** | Versions | V01–V09, V12–V15 | Data history integrity |
| **P1** | Persistence | D01–D09, D11 | File safety |
| **P1** | Search | S01–S04, S06 | Search correctness |
| **P1** | Degradation | L01–L06 | Backend thresholds |
| **P1** | UI | U01–U14 | App shell correctness |
| **P1** | **[GAP]** | G07–G12 | Less critical bugs |
| **P2** | Presence | PR08–PR11 | Edge cases |
| **P2** | Network | N12, N13, N15, N17, N18 | Edge cases |
| **P2** | CRDT | C06, C07, C08, C10, C12 | Deeper correctness |
| **P2** | Security | SEC10–SEC15 | Hardening |
| **P2** | Persistence | D10, D12 | Hardening |
| **P2** | Versions | V10, V11 | Edge cases |
| **P2** | UI | U15 | Nice-to-have |
| **P2** | Search | S05 | Staleness edge case |

---

## Appendix: Test Helpers Needed

### `helpers/app.ts` (Playwright)
```typescript
export async function launchApp(opts?: { notesDir?: string }): Promise<Page>
export async function teardownApp(page: Page): Promise<void>
```

### `helpers/peer.ts` (Playwright)
```typescript
export async function launchTwoPeers(): Promise<{ alice: Page; bob: Page }>
export async function teardownTwoPeers(alice: Page, bob: Page): Promise<void>
export async function waitForSync(alice: Page, bob: Page, docId: string): Promise<void>
export async function disconnectPeers(alice: Page, bob: Page): Promise<void>
export async function reconnectPeers(alice: Page, bob: Page): Promise<void>
```

### `helpers/fixtures.ts` (Playwright)
```typescript
export async function createProject(page: Page, name: string): Promise<string>
export async function createNote(page: Page, project: string, path: string): Promise<string>
export async function typeInEditor(page: Page, text: string): Promise<void>
export async function getEditorText(page: Page): Promise<string>
export async function getDocText(page: Page, project: string, docId: string): Promise<string>
```

### `test_helpers/` (Rust)
```rust
pub fn make_doc_store() -> DocStore
pub fn make_two_connected_endpoints() -> (Endpoint, Endpoint)
pub fn make_manifest(owner: &str, peers: &[(&str, PeerRole)]) -> ProjectManifest
pub fn words(n: usize) -> String // generates exactly n whitespace-separated words
pub async fn sync_until_convergence(store_a: &DocStore, store_b: &DocStore, doc_id: &DocId)
```

---

*Document generated: 2026-03-26. Based on full sequential read of all source files in `/Users/tim/Desktop/notes` by three parallel subagents with domain specializations: (1) frontend/editor/UX, (2) security/backend/crypto, (3) distributed systems/CRDT/sync.*
