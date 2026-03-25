# Research: Innovative Approaches to Version History & Document Timelines

> Research compiled for the P2P Notes App project. Focus on creative, non-standard approaches to version history, undo/redo, and temporal navigation.

---

## 1. Visual Timelines & Scrubbing

### Figma Version History
- **URL**: https://help.figma.com/hc/en-us/articles/360038006754
- **How it works**: Figma auto-saves checkpoints every 30 minutes. Users can browse a timeline of versions in a sidebar, click any version to see a frozen snapshot of the canvas at that point. They can pan around, select/copy assets, and export from any historical version.
- **Key innovation**: **Non-destructive restore** — restoring a previous version doesn't delete the current state; it creates a *new* checkpoint. This means you can always go "forward" again. Also supports **duplicating a version** into a separate file (great for handoff).
- **Branching support**: Figma now has actual branching — you can create branches from a file, work independently, and merge back. This is separate from version history but complements it.
- **Limitation for our purposes**: Still fundamentally a "list of snapshots" — no continuous scrubbing or replay.

### Video-Editor-Style Timeline (Concept)
- The video editing metaphor (After Effects, DaVinci Resolve) uses a **horizontal timeline with a playhead**. Scrubbing the playhead shows the state at any point in time.
- **Nobody has shipped this for text documents in production**, but the concept is powerful: imagine a horizontal bar representing a document's life, with colored segments showing periods of active editing by different authors. Drag the playhead to see the document at that moment.
- The closest implementation is **Draftback** (see below).

### Patchwork "Dynamic History" (Ink & Switch)
- **URL**: https://www.inkandswitch.com/patchwork/notebook/03/
- **Key innovation**: "Dynamic history" — auto-save every character typed, then offer **flexible views of the history depending on the task**. Users can zoom in (single author, small edits) or zoom out (days of work, multiple authors).
- **Implementation insight**: Different groupings of the same underlying changes — by time window, by author, by section. A small set of default groupings (author + edit time) covers most cases, with power-user access to more flexible querying.
- **Built on Automerge**, which tracks full change history natively — no separate versioning layer needed.

---

## 2. Document Replay / Playback

### Draftback (Chrome Extension for Google Docs)
- **URL**: https://github.com/jamiebuilds/draftback (by James Kyle / Jamie Builds)
- **How it worked**: A Chrome extension that accessed Google Docs' internal revision history API (which stores every keystroke) and replayed the entire writing process as a video. You could watch a document being written character by character, like watching a screen recording — but reconstructed from the revision data.
- **UX**: A playback bar with play/pause/scrub controls. Speed controls to fast-forward through boring parts. The document appeared to type itself in real-time.
- **Key insight**: Google Docs internally stores revisions at an extremely granular level (essentially every few keystrokes). Draftback exploited this by fetching all revisions via the undocumented API and replaying them sequentially.
- **Cultural impact**: Went viral when people used it to watch how famous documents (like popular blog posts) were written. Writers used it for self-reflection on their writing process. Teachers used it to verify student work wasn't plagiarized.
- **Status**: No longer maintained / likely broken by Google Docs API changes. But the *concept* remains one of the most innovative document history UX ideas ever shipped.
- **What we can learn**: With Automerge tracking every change, we have the raw data to build this. The key UX challenge is making replay *useful* rather than just a novelty — speed controls, jump-to-interesting-moments, and filtering by author would be essential.

### WritingStreak / Other Writing Apps with Replay
- Several writing apps have experimented with "writing session replay":
  - **750words.com**: Tracks keystroke timing and shows writing speed over time as sparkline graphs
  - **Write or Die**: Not replay per se, but temporal pressure as a writing mechanic
  - **iA Writer**: Shows reading time estimates but no replay
- None have achieved the fidelity of Draftback's full replay approach.

### Draft (draftin.com)
- **Key feature**: "Hemingway Mode" — you can't delete, only write forward. This creates interesting implications for version history since every draft is purely additive.
- **Version history approach**: Every time you start a new editing session, Draft creates a new version. Versions are named by date/time. You can compare any two versions with a diff view.
- **Mark to Me / Sharing**: Draft lets you share a document and track who changed what, with per-collaborator diff views.
- **Unique feature**: **"Transcript mode"** — Draft keeps a log of all text you wrote including deleted passages, preserving the *process* not just the *product*.

---

## 3. Branching and Forking for Writers

### Patchwork "Simple Branching" (Ink & Switch)
- **URL**: https://www.inkandswitch.com/patchwork/notebook/06/
- **The most thorough exploration of Git-like branching for prose writers.**
- **Key design decisions**:
  - Fast, low-ceremony branch creation (no naming required upfront)
  - Branches only from main (no branches-of-branches) — keeps mental model simple
  - **Retroactive branching**: You can start editing, then *later* decide to move those edits to a branch. A hover preview shows what changes would move.
  - AI auto-naming of branches after creation
  - Side-by-side view or diff view to compare branch vs. main
  - Merging deletes the branch (no cleanup step)
- **Finding**: Writers genuinely benefit from branches for:
  - Trying experimental directions without risk
  - Sending a link to a branch for review (like a PR)
  - Individual use (exploring alternatives)

### Upwelling "Layers and Drafts" (Ink & Switch)
- **URL**: https://www.inkandswitch.com/upwelling/
- **Alternative to branching**: Instead of Git-style branches, Upwelling uses "layers" that float on top of a "stack" (the main document).
- **Key innovation**: **Floating drafts** — when one draft is merged, all other drafts are automatically rebased on top of the latest stack. This prevents drafts from diverging too far and surfaces conflicts early.
- **"Formality on demand"**: Writers don't have to decide upfront whether they're "suggesting" or "editing" (unlike Google Docs). They just edit. Later, they can retroactively group and label their changes.
- **Finding**: The "fishbowl effect" — writers feel watched when collaborating in real-time. Layers/drafts provide "creative privacy" — you can work alone, then share when ready.

### Scrivener Snapshots
- **URL**: https://www.literatureandlatte.com/scrivener/features
- **How it works**: Before revising a section, take a manual "Snapshot" — a frozen copy of that section's text. You can take unlimited snapshots per section. Later, you can view any snapshot and use "Compare" to see a diff between the snapshot and the current version.
- **Key design**: Snapshots are **per-section, not per-document**. Since Scrivener already breaks documents into sections (scenes, chapters), this granularity makes sense. You might snapshot Chapter 3 before a major rewrite while leaving other chapters untouched.
- **Limitation**: No automatic snapshots. No branching or merging. No collaborative features. But the *per-section* granularity is a good insight.
- **What we can learn**: The per-section snapshot concept maps well to our per-file Automerge documents. Each note could have its own snapshot timeline independent of others.

### Notion / Google Docs
- **Notion**: Has page-level version history (30 days on free, unlimited on paid). Purely chronological list of snapshots. Can restore any version. No branching, no diff visualization, no per-section granularity.
- **Google Docs**: Named versions, suggestion mode, and version history. The version history groups changes by author with color coding. Still fundamentally a linear timeline.

---

## 4. Collaborative History Visualization

### Patchwork Diff Visualizations
- **URL**: https://www.inkandswitch.com/patchwork/notebook/04/
- **Most innovative diff work I found anywhere.** Key experiments:
  - **Hover-to-show-deleted**: Instead of noisy strikethrough, show a small backspace glyph (⌫). Hover over it to see what was deleted. Much cleaner than Google Docs' suggestion mode.
  - **Margin replacements**: Show "before → after" in the document margin for replaced text. Keeps the main text clean.
  - **Summary diff visualizations**:
    - **Stats**: Word/sentence counts added/removed (not character counts — those aren't meaningful for prose)
    - **Blobs**: Visual representation where contiguous paragraph additions show as large circles, small edits as satellites. Red/green for add/delete.
    - **Minibar**: A horizontal bar showing *where* in the document changes occurred, with section headers for orientation. Most useful for long documents.
    - **Section stats**: Changes broken down by document section with +added/-removed per section.
  - **Team favorites**: Hover-to-show-deleted (for full document) and minibar (for summary).

### Patchwork "Edit Groups"
- **URL**: https://www.inkandswitch.com/patchwork/notebook/05/
- **Key innovation**: After editing, you can **retroactively group related edits together** and supply rationale. Groups can be spatially scattered (e.g., a terminology change across the whole document) or localized.
- **Design principle**: "Formality on demand" — no branches, no suggestion mode. Just edit directly. Then optionally explain and group your edits after the fact.
- **Insight**: This is like Git commits but *after the fact* — you write freely, then package your changes with context. Much more natural for writers than the developer workflow of "think about what to commit" upfront.

### Patchwork "Version History as Chat"
- **URL**: https://www.inkandswitch.com/patchwork/notebook/09/
- **Most innovative UX concept in this research.** Unifies:
  - A **document history timeline** (auto-grouped changes)
  - A **chat interface** for discussing edits
  - **AI-generated summaries** of each batch of changes
  - **Slash commands** for version control operations (create branch, merge, mark milestone)
  - Merged branches appear as single items in the timeline
  - Users can leave informal annotations on the history
- **Finding**: "Brief AI summaries are a remarkably useful way to understand writing edits at a high level — more successful than any of our other diff visualizations."
- **Finding**: Branch merges as single timeline items "encourages us to use branches as a unit to encapsulate work."
- **What we can learn**: This is the most compelling version history UX I've seen. It combines the best of Git log, PR comments, Slack, and AI summarization into a single sidebar. Strongly consider this pattern.

### Upwelling Change Visualization
- **URL**: https://www.inkandswitch.com/upwelling/
- Uses **author-colored highlighting** for insertions and a **proofreader's deletion mark (➰)** instead of strikethrough for deletions.
- Hover over ➰ to see deleted text — same pattern as Patchwork's hover-to-show-deleted.
- **Always-on change tracking**: Unlike Google Docs where you must enable "suggestion mode," Upwelling tracks everything automatically. You choose *when* to visualize changes, not whether to track them.

---

## 5. Ink & Switch Research Projects

### Peritext — CRDT for Rich Text
- **URL**: https://www.inkandswitch.com/peritext/
- Not directly about history UI, but critical foundation: Peritext defines how rich text formatting merges correctly across async collaboration. The branching model shown in their diagram — where document history diverges and converges — is core to understanding how history visualization should work.
- **Key visual**: Their diagram contrasting Google Docs' single linear timeline vs. Git-like branching/merging model is the conceptual foundation for everything in Patchwork and Upwelling.

### PushPin — P2P Collaborative Corkboard
- **URL**: https://www.inkandswitch.com/pushpin/
- Key concepts relevant to our app:
  - **Document FRP (Functional Reactive Programming)**: The render function takes an Automerge document as input. Whether the update comes from local input or remote sync, same code path. Elegant model.
  - **Ephemeral vs. persistent state**: Cursor positions, typing indicators = ephemeral (via gossip). Document content = persistent (via CRDT). Exactly the split we have in our architecture.
  - **Storage peers**: Always-on nodes that replicate data. Similar to our relay concept but with data persistence.

### Cambria — Schema Evolution with Lenses
- **URL**: https://www.inkandswitch.com/cambria/
- Relevant to version history: Cambria solves the problem of different *versions of an app* collaborating on the same document. A v1 client and v2 client with different schemas can interoperate via bidirectional "lenses."
- **For our app**: This is the version *migration* problem, not version *history*. But the insight that multiple schema versions must coexist is important — our `schemaVersion` field in Automerge documents needs a migration strategy.

### Patchwork — "Beyond Prose" Generalization
- **URL**: https://www.inkandswitch.com/patchwork/notebook/10/
- Ported version control concepts to:
  - **Diagram editor** (tldraw): Green glow for added shapes, ghost effect for deleted. Side-by-side comparison views.
  - **Spreadsheets** (Handsontable): Branches as "what-if scenarios" — powerful for financial modeling.
- **Finding**: "Branching and timeline are low-effort to add if you've already built your app on Automerge." Diff view and comments are more domain-specific.
- **Finding**: "For each case where we've applied branching to a new domain, we've quickly found useful ideas. This suggests branching is a powerful general primitive for all creative work."

### Patchwork — "Universal Comments"
- **URL**: https://www.inkandswitch.com/patchwork/notebook/11/
- Abstracted commenting system that works across document types via "pointers" — app-specific references to commentable regions (text spans, spreadsheet cells, drawing shapes).
- **Insight**: Comments, diff highlighting, and search results all use the same "pointer" abstraction. A universal primitive.

### Patchwork — "AI Bots in Version Control"
- **URL**: https://www.inkandswitch.com/patchwork/notebook/07/
- AI edits go on a **branch**, just like human edits. You review and merge (or discard) AI suggestions the same way you'd review a collaborator's work.
- The AI bot appears as a user in the document history timeline.
- Bot prompts are themselves documents that can be versioned, shared, and branched.

### Beckett — Version Control for Students
- **URL**: https://www.inkandswitch.com/universal-version-control/
- Version control for students learning Godot game engine. Exploring how non-developers can benefit from version control concepts.

### Jacquard — Version Control for Scientists
- **URL**: https://www.inkandswitch.com/universal-version-control/
- Working with empirical scientists to explore how academic paper writing can inform collaboration approaches.

---

## 6. Time Machine & Temporal Navigation UX

### Apple Time Machine (The OG)
- The literal "fly through time" metaphor — your desktop recedes into a starfield, and you scrub through snapshots. While technically just browsing backups, the *spatial-temporal metaphor* was revolutionary.
- **What worked**: Making time feel physical. Scrubbing forward/backward with visual continuity.
- **What didn't**: Scaled poorly. Finding a specific change was needle-in-haystack.

### Wayback Machine (Internet Archive)
- **URL**: https://web.archive.org
- Calendar-based temporal navigation: pick a date, see the web as it was. The calendar view with colored dots showing snapshot density is a good pattern for showing *when* a document was actively edited.

### Git Time-Lapse View (Various IDEs)
- JetBrains IDEs have "Annotate with Git Blame" — per-line coloring by recency (warmer = more recent) or by author. You can see at a glance which parts of a file are ancient vs. recently touched.
- **GitHub**: The "blame" view and "history for this file" features. The heatmap contribution graph (green squares) is an iconic temporal visualization.

### Potential "Time Machine" UX for Our App
Based on this research, the most promising approach would combine:
1. **Minibar/sparkline** on each note showing edit density over time
2. **Scrubbing timeline** in history panel (not just a list of versions)
3. **Author-colored heatmap** overlaid on document text showing recency/authorship
4. **Playback mode** for replaying the writing process (powered by Automerge's full change log)

---

## 7. Heat Maps & Activity Visualization

### GitHub Contribution Graph
- The green squares calendar heatmap. Simple, iconic, and immediately readable. Maps to: "show me a small calendar-like grid for each note showing days with edits."

### Code Climate / SonarQube "Hotspots"
- Files that change frequently are marked as "hotspots" — areas of high churn. Could apply to notes: which sections of a document get rewritten most often?

### Potential Approaches for Our App
- **Document heatmap**: Overlay on the text itself, with background color intensity showing how frequently a paragraph has been edited. Highly edited paragraphs glow warm; stable text is cool/neutral.
- **Section sparklines**: In the file tree, show a tiny sparkline next to each file showing edit activity over the past N days.
- **Author attribution heatmap**: Background color by author — see at a glance who wrote what (like Git blame but for prose).
- **Temporal sparkline in status bar**: Show the document's edit velocity — are edits accelerating (deadline approaching?) or stable?

---

## 8. Summary: Top Recommendations for Our App

### Must-Have (Phase 5)
1. **Session-grouped history** (already planned) — Automerge changes grouped by author + time gap into "sessions"
2. **Block-level diffs** (already planned) — added/removed/changed blocks with color coding
3. **Non-destructive restore** — restoring creates a new change, not a revert

### Should-Have (Phase 5 or v2)
4. **Hover-to-show-deleted** — instead of noisy strikethrough, show ⌫ glyph with hover reveal (Patchwork pattern)
5. **Minibar summary** — horizontal bar showing where in the document changes occurred
6. **AI-generated change summaries** — "Alice rewrote the introduction and fixed typos throughout" (Patchwork finding: most useful history feature)
7. **History-as-chat sidebar** — unify history timeline with discussion (Patchwork's most innovative UX)

### Aspirational (v2+)
8. **Document replay/playback** — Draftback-style "watch the document being written" mode
9. **Simple branching for writers** — Patchwork/Upwelling-style lightweight branches with "creative privacy"
10. **Author attribution heatmap** — per-paragraph background coloring by author/recency
11. **Per-note sparklines** in file tree showing edit activity
12. **Retroactive edit grouping** — package changes with context *after* writing, not before

---

## Sources & URLs

| Source | URL |
|--------|-----|
| Figma Version History | https://help.figma.com/hc/en-us/articles/360038006754 |
| Draftback (GitHub) | https://github.com/jamiebuilds/draftback |
| Patchwork Lab Notebook | https://www.inkandswitch.com/patchwork/notebook/ |
| Upwelling | https://www.inkandswitch.com/upwelling/ |
| Peritext | https://www.inkandswitch.com/peritext/ |
| PushPin | https://www.inkandswitch.com/pushpin/ |
| Cambria | https://www.inkandswitch.com/cambria/ |
| Ink & Switch (Main) | https://www.inkandswitch.com/ |
| Ink & Switch Universal Version Control | https://www.inkandswitch.com/universal-version-control/ |
| Scrivener Features | https://www.literatureandlatte.com/scrivener/features |
| NYT Oak Editor | https://open.nytimes.com/building-a-text-editor-for-a-digital-first-newsroom-f1cb8367fc21 |
