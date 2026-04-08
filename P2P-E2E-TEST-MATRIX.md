# P2P E2E Test Matrix

> A concrete end-to-end test plan for the peer-to-peer networking surface of the app.
> This plan is intentionally E2E-first: it focuses on real multi-app behavior to expose missing wiring, race conditions, and user-visible gaps between the frontend, Tauri layer, and Rust backend.

---

## Goals

- Verify that real users can share, join, sync, reconnect, and collaborate across separate app instances.
- Catch gaps that lower-level tests miss: missing Tauri wiring, stale UI state, async races, incomplete background tasks, and misleading status indicators.
- Make failures easy to diagnose by attaching logs, screenshots, and temp app data snapshots.

---

## Test Philosophy

This plan treats the app as a product, not just a library.

Primary assertions should be visible truths:

- a project appears
- a note opens
- note content changes on the other peer
- sync state changes in the UI
- peer state changes in the UI
- viewer editing is blocked
- removed peers stop receiving new content

The suite should not primarily assert internal function calls unless the UI cannot express the truth clearly.

---

## Harness Shape

### Main runner

- WebdriverIO drives real Tauri app windows
- Each test launches 2 or 3 app processes
- Each app process gets:
  - its own temp notes directory
  - its own temp local storage / webview state
  - its own identity / keys
  - deterministic env vars where possible

### App roles

- `owner`
- `editor`
- `viewer`

### Standard topologies

- `2-app`: owner + invitee
- `3-app`: owner + editor + viewer
- `restart`: owner + invitee, then restart one or both

### Required artifacts on failure

- screenshot for each app window
- frontend console logs for each app
- backend logs for each app
- preserved temp project directory snapshot path

---

## Harness Helpers

These helpers should exist before broad test implementation starts.

### Process helpers

- `launchApp({ name, notesDir, env })`
- `closeApp(app)`
- `restartApp(app)`
- `launchPair()`
- `launchTrio()`

### UI helpers

- `createProject(app, name)`
- `openProject(app, name)`
- `createNote(app, path)`
- `openNote(app, title)`
- `typeInEditor(app, text)`
- `replaceEditorText(app, text)`
- `readEditorText(app)`
- `openShareDialog(app)`
- `generateInvite(app, role)`
- `joinProject(app, { passphrase, ownerPeerId })`
- `waitForProjectVisible(app, name)`
- `waitForNoteVisible(app, title)`
- `waitForEditorText(app, text)`
- `waitForSyncState(app, state)`
- `waitForPeerStatus(app, peerId, state)`

### Network helpers

- `disconnectAppNetwork(app)`
- `reconnectAppNetwork(app)`
- `disconnectAllPeers()`
- `reconnectAllPeers()`

### Diagnostics helpers

- `collectFrontendLogs(app)`
- `collectBackendLogs(app)`
- `snapshotNotesDir(app)`
- `bundleFailureArtifacts(testName, apps)`

### Stability / readiness helpers

- `waitForAppReady(app)`
- `waitForBackendReady(app)`
- `waitForIdentityLoaded(app)`
- `waitForProjectHydrated(app, name)`
- `waitForNoteHydrated(app, title)`

### Test-mode controls

These are worth adding to the app specifically for stable E2E:

- override invite TTL
- override large-doc thresholds
- override reconnect / monitor intervals
- deterministic network partition controls
- stable `data-testid` coverage for sync state, peer state, read-only state, join result, and hydration state

---

## Execution Strategy

Implement the suites in this order:

1. Share, join, and initial hydration
2. Note lifecycle propagation (create, rename, delete)
3. Bidirectional editing
4. Offline and reconnect
5. Invite robustness
6. Permissions and removal
7. Restart and rehydration
8. Presence and peer UI
9. Large-document collaboration
10. Multi-project and isolation edge cases

This order surfaces the biggest missing product wiring first.

---

## Suite 1 - Share And Join

### P2P-E2E-001: Owner shares project, invitee joins, project appears

**Priority:** P0

**Apps:** 2 (`owner`, `invitee`)

**Setup:**
- Owner launches app
- Owner creates project `alpha`
- Owner creates note `welcome.md`
- Owner types `hello from owner`

**Steps:**
1. Owner opens Share dialog
2. Owner generates `editor` invite
3. Capture passphrase and owner peer ID from UI
4. Invitee opens Join dialog
5. Invitee enters passphrase and owner peer ID
6. Wait for success state
7. Wait for project `alpha` to appear in invitee sidebar

**Assertions:**
- Invitee sees project `alpha`
- Join success screen shows assigned role `editor`
- No error banner remains visible

**Likely current gaps surfaced:**
- Project may appear before content hydration really finishes
- Background initial sync may lag behind join success

---

### P2P-E2E-002: Joined peer sees owner note content

**Priority:** P0

**Apps:** 2

**Setup:** after successful join from test 001

**Steps:**
1. Invitee opens project `alpha`
2. Invitee opens `welcome.md`
3. Wait for editor to load

**Assertions:**
- Editor contains `hello from owner`
- Invitee can open note without reload or app restart

**Likely current gaps surfaced:**
- Placeholder docs may exist before actual doc content sync completes

---

### P2P-E2E-003: Joined peer survives app restart and still sees shared project

**Priority:** P1

**Apps:** 2

**Setup:** invitee has already joined `alpha`

**Steps:**
1. Restart invitee app
2. Wait for app ready state
3. Open project `alpha`
4. Open `welcome.md`

**Assertions:**
- Project `alpha` still exists
- Note content still matches synced content
- No duplicate project created

---

## Suite 2 - Note Lifecycle Propagation

### P2P-E2E-005: Remote note creation appears without restart

**Priority:** P0

**Apps:** 2

**Steps:**
1. Owner creates a new note `ideas.md`
2. Wait for invitee sidebar refresh

**Assertions:**
- Invitee sees `ideas.md` appear without restarting or reopening project

**Likely current gaps surfaced:**
- Manifest-affecting operations may not be proactively synced like content edits

---

### P2P-E2E-006: Remote note rename appears without restart

**Priority:** P1

**Apps:** 2

**Setup:** note `ideas.md` exists on both peers

**Steps:**
1. Owner renames `ideas.md` to `brainstorm.md`
2. Wait for invitee sidebar refresh

**Assertions:**
- Invitee stops seeing `ideas.md`
- Invitee sees `brainstorm.md`
- Opening renamed note works

---

### P2P-E2E-007: Remote note deletion disappears without restart

**Priority:** P1

**Apps:** 2

**Setup:** note exists on both peers

**Steps:**
1. Owner deletes note
2. Wait for invitee sidebar refresh

**Assertions:**
- Invitee no longer sees deleted note
- If deleted note was open, invitee is returned to a safe UI state

---

## Suite 3 - Real Bidirectional Sync

### P2P-E2E-010: Owner edit appears on invitee without reload

**Priority:** P0

**Apps:** 2

**Setup:** both apps open `welcome.md`

**Steps:**
1. Owner app types ` owner update`
2. Wait for invitee editor to update

**Assertions:**
- Invitee editor shows full merged content
- Invitee did not need to manually reopen note
- Final visible sync state is healthy / synced

**Likely current gaps surfaced:**
- Remote change event may fire but active editor content may not refresh correctly
- Sync indicator may claim synced too early

---

### P2P-E2E-011: Invitee edit appears on owner without reload

**Priority:** P0

**Apps:** 2

**Steps:**
1. Invitee types ` from invitee`
2. Wait for owner editor to update

**Assertions:**
- Owner editor shows invitee changes
- No duplicate text or bounce-back loop

---

### P2P-E2E-012: Sequential edits across two notes sync correctly

**Priority:** P1

**Apps:** 2

**Setup:** create `second.md` in owner app

**Steps:**
1. Owner edits `welcome.md`
2. Invitee edits `second.md`
3. Both peers switch between notes
4. Wait for both notes to converge on both peers

**Assertions:**
- Both notes exist on both peers
- Each note contains expected peer edits
- No content is applied to the wrong document

---

### P2P-E2E-013: Concurrent editing on same note converges visibly

**Priority:** P1

**Apps:** 2

**Setup:** both peers open same note, disconnect network briefly to simulate divergence

**Steps:**
1. Disconnect connectivity between peers
2. Owner app types `owner branch`
3. Invitee app types `invitee branch`
4. Reconnect peers
5. Wait for convergence

**Assertions:**
- Both peers end on identical visible text
- Final text contains evidence of both users' edits
- App remains responsive during merge

**Likely current gaps surfaced:**
- Whole-document text replacement may create surprising merge shapes
- UI may not explain concurrent edits at all

---

## Suite 4 - Offline And Reconnect

### P2P-E2E-020: Offline edit queues locally and syncs after reconnect

**Priority:** P0

**Apps:** 2

**Steps:**
1. Disconnect invitee from network
2. Invitee edits `welcome.md`
3. Observe invitee sync indicator
4. Reconnect invitee
5. Wait for owner content update

**Assertions:**
- Invitee can continue editing while disconnected
- Owner receives invitee changes after reconnect
- Final text converges

**Likely current gaps surfaced:**
- UI offline state may not match actual sync state

---

### P2P-E2E-021: Unsent changes count increases while offline

**Priority:** P0

**Apps:** 2

**Steps:**
1. Disconnect invitee
2. Make several edits in invitee app
3. Observe status bar text / unsent count

**Assertions:**
- UI shows `N unsent changes` where `N > 0`

**Expected current result:**
- Likely fails because backend emits `unsent_changes: 0`

---

### P2P-E2E-022: Reconnect returns UI to healthy synced state

**Priority:** P1

**Apps:** 2

**Steps:**
1. Disconnect invitee
2. Make local edit
3. Confirm local-only / offline state in UI
4. Reconnect invitee
5. Wait for synced state

**Assertions:**
- Final state is synced / healthy, not stuck at syncing or local-only

---

### P2P-E2E-023: App restart after offline edit preserves local change and eventually syncs

**Priority:** P1

**Apps:** 2

**Steps:**
1. Disconnect invitee
2. Edit note on invitee
3. Restart invitee while still offline
4. Verify local edit still exists after restart
5. Reconnect invitee
6. Verify owner eventually receives edit

**Assertions:**
- No local data loss on restart
- Sync resumes after reconnect

---

## Suite 5 - Invite Robustness

### P2P-E2E-030: Wrong invite code shows useful error

**Priority:** P0

**Apps:** 2

**Steps:**
1. Owner generates invite
2. Invitee enters wrong passphrase with correct owner peer ID

**Assertions:**
- Join fails
- UI shows human-readable error
- No partial project appears in sidebar

---

### P2P-E2E-031: Reused invite code fails on second use

**Priority:** P0

**Apps:** 3 or sequential 2-app runs

**Steps:**
1. Owner generates invite
2. Invitee A joins successfully
3. Invitee B tries same code

**Assertions:**
- Invitee B fails to join
- UI shows one-time / invalid / expired style failure

---

### P2P-E2E-032: Expired invite fails cleanly

**Priority:** P1

**Apps:** 2

**Setup:** test harness should support shortened invite TTL in test mode

**Steps:**
1. Owner generates invite
2. Wait past TTL
3. Invitee attempts join

**Assertions:**
- Join fails
- Error is visible and specific enough
- No partial project created

---

### P2P-E2E-033: Two invitees race same invite code

**Priority:** P1

**Apps:** 3

**Steps:**
1. Owner generates a single invite
2. Editor app and viewer app attempt join at nearly the same time
3. Wait for both results

**Assertions:**
- Exactly one succeeds
- Exactly one fails
- Owner ends with exactly one new peer in peer list

**Likely current gaps surfaced:**
- One-time invite consumption may not be atomic enough under concurrent use

---

### P2P-E2E-034: Repeated wrong invite attempts burn down the code or hit visible rate limit

**Priority:** P0

**Apps:** 2

**Setup:** owner generates one invite code

**Steps:**
1. Invitee submits wrong passphrase repeatedly against the same owner peer ID
2. Continue until the configured limit should be exhausted
3. Attempt correct code after the exhaustion threshold

**Assertions:**
- Wrong attempts are not effectively unlimited
- Invite eventually becomes exhausted or visibly blocked
- Correct code no longer succeeds once the limit is burned

**Expected current result:**
- Likely exposes that wrong-code attempts do not really consume the invite as intended

---

### P2P-E2E-035: Invitee disconnects after receiving payload but before completion

**Priority:** P1

**Apps:** 2

**Setup:** requires harness support to drop invitee at a precise point

**Steps:**
1. Owner generates invite
2. Invitee starts join flow
3. Harness disconnects invitee after payload receipt but before full completion
4. Retry join behavior is observed

**Assertions:**
- Invite is not left in a confusing half-consumed state
- No broken partial project remains on disk or in sidebar
- One-time invite semantics remain coherent

---

## Suite 6 - Permissions And Roles

### P2P-E2E-040: Editor can edit shared note

**Priority:** P0

**Apps:** 3 (`owner`, `editor`, `viewer` optional but can be omitted here)

**Steps:**
1. Owner shares project to editor role
2. Editor joins
3. Editor edits note
4. Owner waits for update

**Assertions:**
- Owner receives editor change
- Editor sees normal editing affordances

---

### P2P-E2E-041: Viewer can open note but cannot edit

**Priority:** P0

**Apps:** 3

**Steps:**
1. Owner shares project to viewer role
2. Viewer joins
3. Viewer opens note
4. Viewer tries typing

**Assertions:**
- Viewer can read content
- Typing does not modify content
- UI is visibly read-only or edit action is blocked

**Likely current gaps surfaced:**
- Viewer flow may be only partially wired in UI or sync layer

---

### P2P-E2E-041B: Viewer continues receiving owner updates after join

**Priority:** P0

**Apps:** 3

**Steps:**
1. Viewer joins shared project
2. Owner edits note after viewer is already connected
3. Wait for viewer update

**Assertions:**
- Viewer receives fresh read-only content updates
- Viewer does not need restart or rejoin to stay current

---

### P2P-E2E-042: Owner removes editor, editor stops receiving new content

**Priority:** P0

**Apps:** 3

**Steps:**
1. Owner, editor fully synced
2. Owner removes editor from project
3. Owner edits note after removal
4. Wait and observe editor

**Assertions:**
- Editor peer status updates to disconnected / removed
- Editor does not receive new content after removal
- Owner and remaining peers still sync normally

**Likely current gaps surfaced:**
- ACL cleanup may be incomplete across already-open docs
- Peer connection may remain open even when role is removed

---

### P2P-E2E-043: Removed editor cannot push edits back into project

**Priority:** P0

**Apps:** 3

**Steps:**
1. Remove editor as in test 042
2. Editor attempts local edit after removal
3. Wait for owner state

**Assertions:**
- Owner does not receive removed editor changes
- Removed editor sees failure or local-only state

---

### P2P-E2E-044: Non-owner cannot make effective owner-controlled sharing changes

**Priority:** P0

**Apps:** 3

**Steps:**
1. Join editor and viewer normally
2. From non-owner app, attempt any available sharing or peer-management action
3. Restart peers if needed and inspect resulting roles / peer list

**Assertions:**
- Non-owner cannot make effective owner-controlled changes
- Peer list and roles remain owner-authoritative from user-visible perspective

---

## Suite 7 - Presence And Peer UX

### P2P-E2E-050: Peer connected status appears in UI after join

**Priority:** P1

**Apps:** 2

**Steps:**
1. Complete join flow
2. Open peer panel on owner and invitee

**Assertions:**
- Each app shows the other peer as connected

---

### P2P-E2E-051: Cursor presence appears on remote peer while both view same note

**Priority:** P1

**Apps:** 2

**Steps:**
1. Both open same note
2. Owner moves cursor to several positions
3. Invitee observes editor decorations / remote cursor UI

**Assertions:**
- Invitee sees owner cursor or remote presence marker

**Expected current result:**
- Likely fails because cross-peer presence gossip is not fully wired

---

### P2P-E2E-052: Active file indicators update when peer switches notes

**Priority:** P1

**Apps:** 2

**Setup:** two notes exist

**Steps:**
1. Owner opens note A
2. Invitee checks owner active-dot on note A
3. Owner switches to note B
4. Invitee checks sidebar again

**Assertions:**
- Active peer dot moves from note A to note B in near real time

**Expected current result:**
- Likely fails because document active-peer mapping is not updated live from presence events

---

### P2P-E2E-053: Peer disconnect updates UI to disconnected state

**Priority:** P1

**Apps:** 2

**Steps:**
1. Both peers connected
2. Kill invitee app
3. Wait for owner peer panel update

**Assertions:**
- Owner sees invitee become disconnected
- Sync state reflects peer loss

---

## Suite 8 - Restart And Rehydration

### P2P-E2E-060: Restart owner, peer reconnects, sync still works

**Priority:** P1

**Apps:** 2

**Steps:**
1. Start with synced owner + invitee
2. Restart owner app
3. Wait for reconnect
4. Invitee edits note
5. Wait for owner update

**Assertions:**
- Owner reconnects to invitee
- Post-restart sync still works normally

---

### P2P-E2E-061: Restart both apps, shared project remains healthy

**Priority:** P1

**Apps:** 2

**Steps:**
1. Start synced pair
2. Restart both apps
3. Wait for both ready states
4. Open shared project on both
5. Edit from one peer
6. Verify sync to the other

**Assertions:**
- Shared project persists on both
- Peer state recovers
- New edits sync after restart

---

## Suite 9 - Large Document Collaboration

### P2P-E2E-070: Shared large doc crosses warning threshold and UI reacts

**Priority:** P1

**Apps:** 2

**Steps:**
1. Owner creates shared doc near warning threshold
2. Owner adds enough text to cross threshold
3. Observe owner and invitee UI

**Assertions:**
- Warning / degraded mode UI appears on both peers

**Expected current result:**
- Likely fails because frontend ignores backend degradation level

---

### P2P-E2E-071: Shared very large doc still syncs eventually without UI lockup

**Priority:** P1

**Apps:** 2

**Setup:** doc above batch-sync threshold

**Steps:**
1. Owner edits doc
2. Wait for invitee update

**Assertions:**
- Large-doc editing remains usable
- Invitee still eventually receives update

**Expected current result:**
- Do not assert exact timing in CI E2E; timing-specific cadence should be covered lower in the stack

---

## Suite 10 - Multi-Project / Removal Edges

### P2P-E2E-080: Same two peers share two projects without cross-project contamination

**Priority:** P2

**Apps:** 2

**Steps:**
1. Owner shares project `alpha`
2. Owner shares project `beta`
3. Invitee joins both
4. Edit notes in both projects from different peers

**Assertions:**
- `alpha` edits never appear in `beta`
- Peer connection reuse does not mix project state

---

### P2P-E2E-081: Remove peer from one project only, other project still syncs

**Priority:** P2

**Apps:** 2

**Setup:** same peer pair joined in `alpha` and `beta`

**Steps:**
1. Owner removes invitee from `alpha`
2. Owner edits note in `alpha`
3. Owner edits note in `beta`

**Assertions:**
- Invitee no longer gets `alpha` updates
- Invitee still gets `beta` updates

---

### P2P-E2E-082: Remove peer from one project, restart both apps, removal still holds

**Priority:** P1

**Apps:** 2

**Setup:** same peers in `alpha` and `beta`; peer removed from `alpha`

**Steps:**
1. Restart both apps
2. Owner creates or edits content in `alpha`
3. Owner edits content in `beta`

**Assertions:**
- Removed peer still cannot receive new `alpha` content after restart
- Removed peer still receives `beta` updates normally

---

## Expected Initial Failures

These tests are especially likely to fail against the current implementation and should be treated as high-value signals, not noise.

- `P2P-E2E-021` unsent changes count while offline
- `P2P-E2E-051` remote cursor presence
- `P2P-E2E-052` live active-file peer dots
- `P2P-E2E-070` large-doc warning / degradation UI
- `P2P-E2E-033` concurrent one-time invite race
- `P2P-E2E-034` repeated wrong invite attempts
- `P2P-E2E-035` pre-completion invite disconnect behavior

---

## Minimal First Batch To Build

If we want the fastest path to useful signal, implement these first 8:

1. `P2P-E2E-001` share and join
2. `P2P-E2E-002` joined peer sees note content
3. `P2P-E2E-005` remote note creation propagates
4. `P2P-E2E-010` owner edit syncs to invitee
5. `P2P-E2E-011` invitee edit syncs to owner
6. `P2P-E2E-020` offline edit syncs after reconnect
7. `P2P-E2E-021` unsent changes count while offline
8. `P2P-E2E-041` viewer cannot edit

That set will quickly tell us whether the P2P product really works in practice.

---

## Suggested File Layout For Implementation

```
tests/e2e/p2p/
  helpers/
    app.ts
    network.ts
    share.ts
    editor.ts
    assertions.ts
    diagnostics.ts
  01-share-and-join.spec.ts
  02-basic-sync.spec.ts
  03-offline-reconnect.spec.ts
  04-invite-robustness.spec.ts
  05-permissions.spec.ts
  06-presence.spec.ts
  07-restart.spec.ts
  08-large-docs.spec.ts
  09-multi-project.spec.ts
```

---

## Success Criteria

This P2P E2E suite is successful when:

- it catches real missing wiring and regressions
- failures explain what the user would experience
- it stays stable enough to run in CI on every PR
- it covers both happy path and ugly path behavior
- we can trust that a shipped networking feature actually works end-to-end
