# P2P Notes App — Architecture Plan (Reviewed & Approved)

> Reviewed by 5 domain experts (P2P/Distributed Systems, Security/Cryptography, Rust/Tauri/Systems, Frontend/UX/Editor, CRDT/Data Sync). All reviewers signed off after 3 rounds of review.

---

## 1. Overview

A local-first, peer-to-peer, end-to-end encrypted markdown notes app. Users create folders locally (each folder is a project), write markdown notes with live WYSIWYG rendering, and optionally invite others to share projects via P2P connections. All data lives on the user's machine. There are no centralized servers — only a thin, stateless relay for NAT traversal.

---

## 2. Tech Stack

| Layer | Technology | Rationale |
|---|---|---|
| App Shell | **Tauri 2** (Rust) | Small binary, native performance, cross-platform desktop |
| Frontend | **Svelte 5 + Vite** | Lightweight, fast. No SvelteKit — SSR is dead weight in Tauri |
| Editor | **TipTap** (ProseMirror) | Company-backed, large plugin ecosystem, proven ProseMirror access |
| CRDT (frontend) | **Automerge WASM** | Instant local editing, zero IPC latency |
| CRDT (backend) | **Automerge Rust** | Sync peer, persistence, history, compaction |
| P2P Networking | **iroh** (Rust) | NAT traversal, relay, blob sync, gossip, built-in E2E encryption |
| Encryption (transport) | **iroh built-in** (QUIC E2E) | Always-on, zero config |
| Encryption (at-rest) | **XChaCha20-Poly1305** | Nonce-misuse resistant, 192-bit random nonces |
| Key Storage | **OS Keychain** + `zeroize` | Keychain Services (macOS), DPAPI (Windows), libsecret (Linux) |

---

## 3. Architecture

```
+------------------------------------------------------+
|  Svelte 5 Frontend (Tauri Webview)                   |
|  +--------------------------------------------------+|
|  |  TipTap Editor (ProseMirror)                     ||
|  |    +-- @automerge/prosemirror bridge             ||
|  |    +-- Automerge WASM (local editing instance)   ||
|  |    +-- Cursor decorations (remote presence)      ||
|  |    +-- ConflictHighlight (concurrent edits)      ||
|  +--------------------------------------------------+|
|  |  Sidebar: file tree + peer presence + sync icons ||
|  |  Status bar: connection dot, unsent changes      ||
|  +--------------------------------------------------+|
|         | Tauri IPC (batched incremental, 100-500ms) |
+---------|--------------------------------------------+
          v
+------------------------------------------------------+
|  Rust Backend (Tauri)                                |
|  +------------------+  +----------------------------+|
|  |  Automerge Rust  |  |  iroh Endpoint             ||
|  |  (sync peer,     |  |  - P2P QUIC connections    ||
|  |   persist,       |<>|  - LocalSwarmDiscovery     ||
|  |   compaction,    |  |  - Relay NAT traversal     ||
|  |   history)       |  |  - Blob sync (images)      ||
|  |                  |  |  - Gossip (presence)        ||
|  |  DocStore:       |  +----------------------------+|
|  |  DashMap<DocId,  |                                |
|  |   Arc<RwLock>>   |                                |
|  +--------+---------+                                |
|           v                                          |
|  +--------------------------------------------------+|
|  |  Local Storage                                   ||
|  |  ~/Notes/<project>/                              ||
|  |    .p2p/automerge/   (CRDT docs, encrypted)     ||
|  |    .p2p/manifest.automerge (project metadata)   ||
|  |    .p2p/keys/        (OS keychain backed)       ||
|  |    *.md              (READ-ONLY exports)        ||
|  |    assets/           (images via iroh blobs)    ||
|  +--------------------------------------------------+|
+------------------------------------------------------+

+-----------------------------+
|  Relay Servers (2+ VPS)     |
|  iroh-relay (stateless)     |
|  + iroh public relay backup |
+-----------------------------+
```

### Data Flow

1. User types in TipTap -> `automerge-prosemirror` -> Automerge WASM (instant, no IPC)
2. Batched incremental changes sent to Rust backend every 100-500ms via Tauri IPC
3. Frontend WASM <-> Rust backend use Automerge sync protocol (they are local peers)
4. Rust backend syncs with remote peers via iroh QUIC connections
5. Remote changes: iroh -> Rust Automerge -> Tauri event -> Frontend WASM -> TipTap updates
6. Presence/cursors: iroh gossip (ephemeral, throttled 10/sec) -> ProseMirror decorations

---

## 4. Data Model

### Folder Structure

```
~/Notes/
+-- personal/                     <- Local-only project
|   +-- journal.md
|   +-- ideas.md
+-- startup-team/                 <- Shared project (P2P)
|   +-- .p2p/
|   |   +-- manifest.automerge   (project metadata CRDT)
|   |   +-- automerge/           (CRDT document store, encrypted)
|   |   +-- keys/                (encryption keys, OS keychain backed)
|   +-- roadmap.md               (READ-ONLY export)
|   +-- meeting-notes/
|   |   +-- 2026-03-24.md
|   +-- assets/
|       +-- diagram.png
```

### Project Manifest (`manifest.automerge`)

```json
{
  "schemaVersion": 1,
  "projectId": "uuid-v4",
  "name": "startup-team",
  "created": "2026-03-24T10:00:00Z",

  "_ownerControlled": {
    "owner": "iroh-node-id-of-owner",
    "peers": {
      "node-id-abc": { "role": "editor", "alias": "Alice", "since": "..." },
      "node-id-def": { "role": "viewer", "alias": "Bob", "since": "..." }
    },
    "keyEpoch": 2,
    "epochKeys": {
      "node-id-abc": "<wrapped-epoch-key-for-alice>",
      "node-id-def": "<wrapped-epoch-key-for-bob>"
    },
    "sharing": { "group": "single" }
  },

  "files": {
    "uuid-1": { "path": "roadmap.md", "created": "...", "type": "note" },
    "uuid-2": { "path": "meeting-notes/2026-03-24.md", "created": "...", "type": "note" }
  }
}
```

**Key invariants:**
- `_ownerControlled` fields: only changes signed by Owner's Ed25519 key are accepted
- `files` fields: any Editor can create/rename/delete files
- Cross-document references use stable UUIDs, not file paths
- Renames only change the manifest mapping; links using UUIDs never break
- UUID collisions: impossible (UUIDv4)
- Path collisions: resolved by suffix `-1`, `-2`; user notified

### Automerge Document Schema

Each `.md` file is backed by an Automerge document:
- Rich text content using Automerge's marks and block markers
- `schemaVersion` field for migration support
- Migrations are idempotent, monotonic, additive-only
- Images stored as iroh blob hash references (~32 bytes), not binary data

**Source of truth**: Automerge documents. `.md` files are read-only exports.

---

## 5. Editor Design

### TipTap Extensions (v1)

**Core:** `StarterKit` (Bold, Italic, Strike, Code, Heading, BulletList, OrderedList, Blockquote, HorizontalRule, HardBreak — with History *disabled*)

**Rich content:** `Image`, `Link`, `CodeBlockLowlight` (CodeMirror 6 embedded), `Table`, `TableRow`, `TableCell`, `TableHeader`, `TaskList`, `TaskItem`

**Collaboration:** `Collaboration` (Automerge bridge via `@automerge/prosemirror`), `CollaborationCursor` (custom, using iroh gossip)

**UX:** `Placeholder`, `Typography`, `Dropcursor`, `Gapcursor`

**Custom:** `MarkdownSource` (toggle raw markdown view), `ConflictHighlight` (concurrent edit detection)

### Editor Behavior
- Hybrid WYSIWYG: `**bold**` input produces bold text immediately. Users never see raw markdown
- Toolbar and slash commands for formatting
- "Source mode" toggle as power-user escape hatch
- Code blocks: CodeMirror 6 embedded in ProseMirror with syntax highlighting
- Images: rendered inline, served from filesystem via Tauri asset protocol (not data URLs)
- Undo/redo: TipTap's `History` disabled. Automerge change-based undo respects collaboration semantics (undoes only local user's changes)

### Large Document Degradation

| Threshold | Action |
|---|---|
| 10k words | Warning banner suggesting split |
| 15k words | Disable collaboration decorations (remote cursors, presence highlights). Block-level-only syntax highlighting |
| 20k words | Batch-only sync (every 5s instead of real-time). Disable spell check |
| 25k+ words | Persistent notice recommending split. All non-essential plugins disabled. "Performance mode" |

---

## 6. P2P Networking

### iroh Endpoint Configuration
- Embedded in Tauri backend via `tauri::async_runtime::spawn`
- Uses Tauri's existing tokio runtime (no second runtime)
- Discovery order: LocalSwarmDiscovery (LAN) -> direct connection -> relay fallback

### Wire Protocol

```
Stream open:
  [1 byte: protocol version (0x01)]
  [1 byte: message type]
  [32 bytes: document ID]

Then repeated:
  [4 bytes: big-endian length]
  [N bytes: Automerge sync message]
```

- One QUIC stream per Automerge document
- Natural multiplexing and backpressure via QUIC flow control
- Sync state persisted after receiving+applying messages (write-ahead pattern)
- Sync health check: reset SyncState after N unproductive round-trips

### Relay Servers
- 2+ self-hosted iroh-relay instances on VPS (different regions)
- iroh public relay (`relay.iroh.network`) as tertiary fallback
- Stateless: no data stored, just facilitates peer connections
- All traffic E2E encrypted (relay cannot read content)

### Peer Identity
- Each device has a persistent Ed25519 keypair (NodeId)
- NodeIds stored in project manifest for reconnection
- Relay serves as rendezvous: NodeId -> current address resolution
- IPs can change freely; peers are identified by cryptographic keys

---

## 7. Sharing & Invite Flow

### PAKE-Based Invite

```
1. Owner clicks "Share" -> generates invite code:
   - 6 random BIP-39 words (~77 bits entropy)
     e.g. "tiger-marble-ocean-violet-canyon-frost"
   - OR 8-char base62 (~47 bits) for short contexts
   - Contains: project ID + relay URL
   - TTL: 5-15 minutes
   - One-time use, tracked by Owner's app
   - Rate-limited: 3 handshake attempts per code

2. Invitee enters code -> iroh connects to Owner via relay

3. Both run SPAKE2 handshake using code as shared secret
   - Prevents MITM even if relay is compromised
   - Produces session-bound shared key

4. Optional SAS (Short Authentication String) visual verification

5. Owner sends (encrypted): project key + manifest
   - Project key wrapped via X25519 ECDH + HKDF + XChaCha20-Poly1305

6. Invitee added to manifest with chosen role

7. Initial sync: manifest first -> Automerge docs -> blobs lazily on-demand
```

---

## 8. Permissions Model

| Role | Read | Edit | Create/Delete Files | Manage Peers | Delete Project |
|---|---|---|---|---|---|
| **Owner** | Yes | Yes | Yes | Yes | Yes |
| **Editor** | Yes | Yes | Yes | No | No |
| **Viewer** | Yes | No | No | No | No |

### Enforcement

- **Cryptographic**: Every CRDT change signed with author's Ed25519 key. Peers validate signatures against ACL.
- **ACL writes**: Owner-only. Only changes to `_ownerControlled` fields signed by Owner's key are accepted. All other peers' ACL changes rejected at validation layer.
- **Viewer isolation**: Viewers receive push-based read-only snapshots over a dedicated one-directional QUIC stream. 500ms debounce. No sync messages, no ability to inject changes.
- **Editor enforcement**: Changes from peers not in the ACL or without matching role are rejected.

### Trust Model (Documented)
- This system protects against curious peers, not against malicious peers with modified clients who already possess the project key.
- A malicious Editor can read all content but cannot forge Owner operations (ACL changes, key rotation).
- Permissions provide meaningful protection against honest-but-unauthorized actions.

---

## 9. Encryption Design

### Transport
- iroh provides E2E encryption over QUIC for all peer connections (always on, zero config)
- Every iroh connection is encrypted; relay cannot read content

### At-Rest
- Algorithm: **XChaCha20-Poly1305** (192-bit random nonces, nonce-misuse resistant)
- Per-document key derived via HKDF-SHA256 from project epoch key
- HKDF context strings:
  - `"p2p-notes/v1/document-encryption" || document_id || epoch`
  - `"p2p-notes/v1/key-wrapping" || epoch || owner_pk || peer_pk`
  - `"p2p-notes/v1/nonce-derivation"` (if deterministic nonces ever needed)
- Salt: random value generated at project creation, stored in manifest
- Keys stored in OS keychain. Zeroized in memory after use (`zeroize` crate).

### Epoch-Based Key Ratcheting

```
Epoch 0: key_0 (created at project creation)
Epoch 1: key_1 (created when peer removed)
Epoch 2: key_2 (created when another peer removed)
```

- New changes encrypted under latest epoch key
- Old documents NOT re-encrypted (forward secrecy only)
- Lazy re-encryption: old docs re-encrypted on next local edit
- Removed peer never receives epoch N+1 key

**Epoch key distribution:**
1. Owner generates epoch N+1 key
2. Owner wraps key individually for each remaining peer:
   - X25519 ECDH (Owner's X25519 + peer's X25519) -> shared secret
   - HKDF with context `"epoch-key-wrap" || epoch_number || owner_pk || peer_pk`
   - XChaCha20-Poly1305 encrypt epoch key
3. Wrapped keys stored in manifest: `epochKeys[peerNodeId] = encrypted_key`
4. Peers unwrap on next manifest sync
5. Offline peers receive wrapped key when they reconnect (manifest is CRDT, eventually consistent)
6. Removed peers find no wrapped key -> cannot decrypt new-epoch content

### Relay Threat Model (Documented)
- Relay is treated as an honest-but-curious adversary
- Relay can infer the social/collaboration graph (who connects to whom)
- Relay can perform traffic analysis (timing, size, frequency)
- Relay cannot read content (E2E encrypted)
- These are accepted risks, not oversights

---

## 10. Collaboration UX

### Real-Time Editing
- Automerge CRDT merges changes automatically
- Cursor presence via iroh gossip (ephemeral, not persisted)
- Cursor updates throttled to 10/sec max

### Remote Cursors
- Colored vertical bar + name label (fades after 3s inactivity)
- Rendered via ProseMirror decorations (not DOM overlays)
- Remote text selection: translucent highlight in user's color

### Presence Indicators
- Sidebar file tree: small avatar dots next to files other peers have open (max 3, then "+N")
- Editor header: row of peer avatars (max 5, then "+N") with online/offline dots

### Concurrent Edit Conflict Detection
- After merging remote changes, detect overlapping edits in same paragraph (same causal gap)
- Surface as highlighted blocks with author attribution
- User can accept merged version or pick one side
- No "conflict" dialogs — subtle "concurrent edits detected" annotations

### Offline UX
- **Green dot**: Connected to peers, syncing normally
- **Yellow dot**: Connected but sync slow (>5s since last peer ack)
- **Gray dot with slash**: No peers connected (fully offline)
- Per-file sync icons in tree: checkmark (synced), spinner (syncing), cloud-off (local only)
- "N unsent changes" in status bar when offline
- Auto-reconnect on network recovery (silent, no manual button)

---

## 11. Version History

- Powered by Automerge's built-in change tracking
- **Session-grouped**: continuous edits by one author with <5min gaps = one session
- Display: "Alice edited on Mar 24 at 2:30pm (45 changes)"
- **Block-level diffs**: added blocks green, removed red, changed yellow
- **Restore**: creates a new Automerge change (not a revert). Confirmation dialog explains impact.
- Deferred to v2: per-character attribution, timeline scrubbing, branching

---

## 12. Concurrency & State Management (Rust Backend)

### Document Store
```rust
use dashmap::DashMap;
use automerge::AutoCommit;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct DocStore {
    docs: DashMap<DocId, Arc<RwLock<AutoCommit>>>,
}
```

- DashMap: lock-free concurrent access to different documents
- tokio RwLock: concurrent reads, exclusive writes per document
- Lazy-load: documents loaded on open, unloaded on close + idle timeout
- Hold DashMap ref minimally: get Arc clone, drop ref, then lock

### Persistence
- Dedicated async save loop per document (debounced, every 5s)
- Atomic writes: write to `.tmp`, then rename
- Corruption recovery: load with permissive options, keep last-known-good backup
- Periodic compaction via `save()` + reload to shed intermediate ops

### Background Tasks
- Supervisor pattern for all background tasks (sync, save, gossip)
- `tokio::JoinSet` for monitoring task completion/panics
- Automatic restart on failure

---

## 13. Build & Project Structure

### Cargo Workspace
```
/src-tauri/          -> Tauri app (depends on automerge, iroh)
/crates/am-wasm/     -> Automerge WASM bindings (wasm-pack)
/crates/notes-sync/  -> Sync protocol crate (Automerge sync over iroh)
/src/                -> Svelte 5 frontend (imports am-wasm)
```

### Build Optimization
- `sccache` or `mold` linker for local dev
- `cargo-chef` for Docker/CI layer caching
- WASM build: `wasm-pack` with `wasm-opt` in release pipeline
- WASM module loaded once at app startup (not per-document)

---

## 14. Development Phases

### Phase 0: PoC Sprint (Weeks 1-2)
- Validate `@automerge/prosemirror` with TipTap
- Test: concurrent formatting (bold, italic, code, links)
- Test: concurrent table editing
- Test: 5k+ word documents with simulated remote changes at 1/sec
- Test: undo/redo across sync boundaries
- Test: Automerge WASM in browser + Rust backend as sync peer
- **Go/no-go decision**: if PoC fails, evaluate Yjs (`y-prosemirror`) as editor layer

### Phase 1: Local Editor (Weeks 3-5)
- Tauri 2 + Svelte 5 + Vite scaffolding
- TipTap editor with live WYSIWYG markdown rendering
- Automerge WASM in frontend, Rust backend as sync peer via Automerge sync protocol
- Local file ops: create/read/update/delete notes and folders
- Project manifest Automerge document
- `.md` export (read-only)
- File tree sidebar with Cmd+P quick-open
- Document compaction and lazy loading

### Phase 2: P2P Foundation (Weeks 6-8)
- iroh endpoint in Tauri backend (LocalSwarmDiscovery + relay)
- `notes-sync` crate: Automerge sync over iroh QUIC with wire protocol
- Peer identity (Ed25519 keypairs, OS keychain storage)
- SPAKE2-based invite flow with TTL and rate limiting
- Document sync between two peers
- Blob sync for images via iroh blobs
- Deploy 2+ relay servers

### Phase 3: Real-time Collaboration (Weeks 9-11)
- Live CRDT sync (changes as you type)
- Cursor/presence via iroh gossip (ephemeral, throttled)
- Remote cursor decorations in TipTap
- Online peer indicators in sidebar and editor header
- Concurrent edit conflict detection + annotation
- Sync status indicators (green/yellow/gray dots)
- "N unsent changes" status bar
- Viewer snapshot delivery (push-based, 500ms debounce, one-directional)

### Phase 4: Security & Permissions (Weeks 12-14)
- Signed CRDT changes (Ed25519) with ACL validation
- Owner-only ACL writes in manifest
- Viewer isolation (read-only snapshots, no sync messages)
- XChaCha20-Poly1305 at-rest encryption with HKDF context strings
- Epoch-based key ratcheting with per-peer key wrapping
- OS keychain integration + `zeroize` for key material
- Threat model documentation

### Phase 5: Polish & Ship (Weeks 15-17)
- Version history UI (session-grouped, block-level diffs, restore)
- Full-text search across notes (SQLite FTS5 via Tauri)
- Settings UI (relay config, display name, theme)
- Keyboard shortcuts
- Large document degradation thresholds
- Error handling hardening (atomic saves, corruption recovery, supervisors)
- Auto-update (Tauri built-in)
- Packaging for macOS, Windows, Linux

---

## 15. Documented Limitations (v1)

1. **No subfolder-level sharing** — one sharing group per project. Deferred to v2.
2. **Forward secrecy only** on peer removal — removed peers retain historical data they already synced.
3. **Deletion is tombstone-based** — deleted content persists in CRDT history. True erasure requires creating a new document.
4. **Relay sees social graph** — which peers connect, when, and how often. Content is E2E encrypted.
5. **`.md` files are read-only exports** — edit in the app only. Manual import available as explicit action.
6. **Document size soft limit** — progressive degradation above 10k words.
7. **Single Owner** — multi-admin ACL deferred to v2 (requires serialized causal ACL changes).
8. **One identity per device** — peer = device installation, not human. Device compromise requires re-joining from other devices.
9. **~20 peer practical limit** per project — full-mesh sync at scale needs gossip topology (v2).
10. **Owner loss** — if the Owner's device is permanently lost, ACL cannot be modified. Recovery mechanism (pre-shared recovery key or signed succession) deferred to v2.

---

## 16. Review History

| Round | P2P/Distributed | Security/Crypto | Rust/Tauri | Frontend/UX | CRDT/Data |
|---|---|---|---|---|---|
| 1 | NEEDS CHANGES | NEEDS CHANGES | NEEDS CHANGES | NEEDS CHANGES | NEEDS CHANGES |
| 2 | LGTM w/ minor | LGTM w/ minor | **LGTM** | LGTM w/ minor | LGTM w/ minor |
| 3 | **LGTM** | **LGTM** | -- | **LGTM** | **LGTM** |

### Key Changes from Reviews
- Moved Automerge to WASM in frontend (eliminated IPC-per-keystroke bottleneck)
- Switched Milkdown -> TipTap (reliability, ecosystem)
- Switched SvelteKit -> Svelte 5 (removed SSR dead weight)
- Eliminated filesystem watcher (`.md` export-only)
- Added cryptographic permission enforcement (signed CRDT changes)
- Added SPAKE2 invite flow (prevents MITM)
- Added epoch-based key ratcheting (no mass re-encryption)
- Switched AES-256-GCM -> XChaCha20-Poly1305 (nonce-misuse resistant)
- Added project manifest Automerge doc with stable UUIDs
- Added Owner-only ACL writes (prevents privilege escalation via CRDT merge)
- Added wire protocol version byte (future-proofing)
- Added progressive large-document degradation strategy
- Added explicit viewer snapshot delivery mechanism
- Documented threat model, trust model, and all known limitations
