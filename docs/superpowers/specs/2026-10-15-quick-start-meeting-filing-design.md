# Quick-Start Meeting and AI Filing Design

Date: 2026-10-15
Status: Draft, awaiting review

## Goal

Pressing the shortcut should start recording at once, with no project to pick. Once the meeting has enough transcript, the assistant files it into the right project (or a new one) and gives it a real title. From then on the observer has that project's brief and sources for the rest of the meeting. The user never has to file meetings by hand, but can always see what the assistant did and change or undo it.

This is sub-project 2 of the transition described in [2026-10-14-handy-to-meet-assist-transition-design.md](2026-10-14-handy-to-meet-assist-transition-design.md#sub-project-context). It replaces the interim "start in the most recently used project" shortcut behaviour.

## Decisions

Settled during brainstorming:

1. **Auto-file, undoable.** The assistant assigns, creates, and renames on its own. The user is told and can change or undo.
2. **Live filing.** Filing runs during the meeting once there is enough transcript, retries if the assistant needs more, and makes one last attempt when the meeting ends.
3. **Floating card plus header picker.** A filed notice appears on the existing floating card (Change, Undo). The meeting detail header has a project picker for manual moves.
4. **Agent tool loop.** The assistant uses filing tools (list, read, assign, create, rename) through the existing tool loop, not a single structured answer. Rust enforces the safety rules.
5. **Undo deletes an empty project** the assistant created in that run; a project with any other content is kept.

## Non-goals

- Filing meetings started from a project page (they are already filed).
- Re-filing a meeting the user has moved, or any meeting already in a project.
- Filing when the assistant is off or its provider is not ready. Such meetings stay unfiled and are filed by hand.
- An MCP server for the filing tools.
- Journey UX beyond the card variant and picker (banners, onboarding, branding). Sub-project 3.

## Flow

1. **Shortcut start.** `meeting::toggle::toggle_meeting` ends a running (live or paused) meeting; otherwise it calls `mm.start(None, None)`: an unfiled meeting with the auto title. `last_used_project` and the "no projects, open Projects" branch are removed. The shortcut works with zero projects. Starting from a project page and Resume are unchanged.
2. **Eligibility.** A meeting is eligible for filing when `project_id IS NULL` and `filed_by IS NULL`.
3. **Live trigger.** On each observer tick (same gates as the observer: `meeting_assistant_enabled`, readiness `Ready`, meeting live), before the judge runs, if the meeting is eligible and:
   - it has at least `FILING_MIN_LINES` (20) transcript lines,
   - fewer than `FILING_MAX_ATTEMPTS` (3) attempts were made for this meeting,
   - the last attempt (if any) was at least 10 lines ago,
   - filing is not marked unavailable for this meeting,

   the observer calls `filing::run`. The judge tick then runs as usual in the same loop iteration; after a successful filing it sees the project.

4. **End trigger.** When a meeting ends, the summary background thread (`spawn_summaries`) first makes one last filing attempt if the meeting is still eligible, has at least 5 lines, and the assistant is ready; then it writes the session summary. Meetings under 5 lines stay unfiled.
5. **Notify.** After a run that filed the meeting, emit `meeting-filed` and show the filed card. No beep.

## Filing module (`meeting/filing.rs`)

Owns the filing prompt, tools, context, and the run.

**Prompt.** System: file this meeting into the project it belongs to, using the tools; prefer an existing project; create one only when none fits; give the meeting a short descriptive title; if the transcript does not yet say what the meeting is about, file nothing and answer `wait`. User: the meeting's current title, its context note, and the transcript so far (most recent lines first-truncated to a character budget, same style as the observer window).

**Run.** `llm_client::send_with_tools_with_client` with the filing tool defs, a `MAX_TOOL_CALLS`-style cap (8), a 30 s budget, and the final schema:

```json
{ "outcome": "filed" | "wait", "reason": "string" }
```

The result of a run is what the tools did, not what the final answer claims: `FilingResult { filed: Option<Filed>, renamed: Option<String> }` read from the context after the loop. `outcome` and `reason` are logged only.

**Context.** `FilingContext { store, meeting_id, state: Mutex<FilingRunState> }` where `FilingRunState` records the project filed into, whether it was created in this run, and the new title.

**Tools** (separate from `tools::tool_defs`; never offered to the observer):

| Tool             | Input                                        | Output                                                                                        |
| ---------------- | -------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `list_projects`  | none                                         | `id`, `title`, brief excerpt (first 300 chars), meeting count, `updated_at` for every project |
| `read_project`   | `project_id`                                 | full brief, source titles with digests, rolling summaries of the 3 most recent meetings       |
| `assign_meeting` | `project_id`                                 | `{ "ok": true, "project_title" }`                                                             |
| `create_project` | `title` (1–80 chars), `brief` (≤ 2000 chars) | `{ "ok": true, "project_id", "reused": bool }`                                                |
| `rename_meeting` | `title` (1–80 chars)                         | `{ "ok": true }`                                                                              |

**Safety rules, enforced in Rust:**

- At most one of `assign_meeting` / `create_project` succeeds per run; later calls return `{"error":"meeting already filed"}`.
- `create_project` reuses an existing project whose title matches case-insensitively after trimming (`reused: true`, not counted as created).
- Every write goes through a conditional store update that only applies while the meeting is still eligible. If the user moved the meeting mid-run, the write returns `{"error":"meeting was filed by the user"}` and the run files nothing.
- `rename_meeting` is allowed once per run and only while the meeting has not been moved by the user.
- Handlers return `{"error": ...}` JSON and never panic, as in `tools.rs`.

**Failure.** `ToolLoopError::ToolsRejected` marks filing unavailable for this meeting (no more live attempts; the end attempt is skipped too). Other errors and timeouts count as an attempt and are logged with `warn!`. No failure is shown in the UI.

## Data

**Migration.** Append to `MIGRATIONS` in `store.rs`:

```sql
ALTER TABLE meetings ADD COLUMN filed_by TEXT;
```

Values: `NULL` (nobody filed it), `'ai'`, `'user'`. Existing rows stay `NULL`; they already have a project, so they are never eligible. `Meeting` gains `filed_by: Option<String>`; `MEETING_COLS` and the bindings are updated.

**Store methods.**

- `file_meeting_ai(meeting_id, project_id) -> Result<bool>`: `UPDATE meetings SET project_id=?, filed_by='ai', updated_at=? WHERE id=? AND project_id IS NULL AND filed_by IS NULL`; returns whether a row changed; touches the project.
- `create_project_and_file_ai(meeting_id, title, brief) -> Result<Option<(Project, bool /*created*/)>>`: one transaction: find a project by trimmed case-insensitive title or insert one, then the same conditional update. Returns `None` (and rolls back an inserted project) if the meeting is no longer eligible.
- `rename_meeting_ai(meeting_id, title) -> Result<bool>`: conditional on `filed_by IS NOT 'user'`.
- `move_meeting(meeting_id, project_id: Option<i64>) -> Result<()>`: unconditional; sets `filed_by='user'`; touches the target project.
- `undo_filing(meeting_id, created_project_id: Option<i64>) -> Result<()>`: one transaction: `move_meeting(meeting_id, None)`, then delete `created_project_id` only if it has no meetings, no sources, and its brief equals the brief the assistant wrote (compared against the stored value at creation, kept in the card payload as `created_brief`). Otherwise the project is kept.

## Session state

`MeetingManager` gains `set_active_project(meeting_id, project_id: Option<i64>)`, which updates `Active.project_id` if `meeting_id` is the live meeting. Both filing and `move_meeting` call it so `state()` and anything reading the cache see the change.

## Commands and events

- `move_meeting(meeting_id: i64, project_id: Option<i64>)`: store move, update the active cache, emit `meeting-filed`.
- `undo_filing(meeting_id: i64, created_project_id: Option<i64>, created_brief: Option<String>)`: store undo, update the active cache, emit `meeting-filed`, hide the card.
- Event `meeting-filed` (`MeetingFiledPayload`): `meeting_id`, `project_id: Option<i64>`, `project_title: Option<String>`, `meeting_title`, `by: "ai" | "user"`, `created_project: bool`, `created_brief: Option<String>`.

## UI

**Floating card.** `intervention::show_filed(app, &MeetingFiledPayload)` emits `intervention:filed` to the existing card window and shows it like a nudge. `InterventionCard` holds a union of a nudge or a filed notice. The filed variant:

- header: `FolderInput` icon, "Filed by assistant", dismiss (X)
- body: "Filed into **{project}** as "{title}"", or "Created **{project}** and filed "{title}"" when `created_project`
- actions: **Change** (`focus_meeting`, which opens the meeting detail with the picker) and **Undo** (`undo_filing`)
- same 30 s auto-hide and hover hold; a later nudge replaces it.

**Meeting detail header.** A project picker using the existing `components/ui` select: "Unfiled" plus all projects. A "Filed by assistant" tag shows when `filed_by == 'ai'`. Changing it calls `move_meeting`.

**Meeting list.** Each row shows its project title, or a muted "Unfiled". A live unfiled, eligible meeting shows "Assistant will file this soon" when the assistant is ready, otherwise "Unfiled · pick a project".

**Stores.** `meetingStore` listens for `meeting-filed`: reload the list, refresh the open detail. `projectStore` reloads projects (a project may have been created or deleted).

**i18n.** New keys under `meetings.filing.*` in `en/translation.json` only.

## Privacy

Filing sends the transcript so far and project titles, briefs, source digests, and meeting summaries to the configured provider. It runs only under the observer's gates (`meeting_assistant_enabled` and readiness), so nothing goes to a provider the observer would not already use. Local providers work the same way.

## Docs

- `docs/ARCHITECTURE.md`: toggle paragraph (starts an unfiled meeting), meeting module table (`meeting/filing.rs`), filing flow.
- `AGENTS.md`: Application Flow step 3 ("starts an unfiled meeting; the assistant files it") and the meeting module list.
- `meeting/toggle.rs` doc comment.

## Testing

Per the minimal test policy, no new UI, end-to-end, or unit tests. The existing store data-safety tests in `store.rs` are extended:

1. The migration adds `filed_by` and leaves existing meetings with their project.
2. `file_meeting_ai` refuses after `move_meeting` (manual move wins the race).
3. `undo_filing` deletes an empty assistant-created project and keeps one that gained a meeting or source.

Gate: `bun run lint`, typecheck (`bun run build`), `cargo clippy`, `cargo test` for the store tests, `bun run format:check`.

Manual checks:

1. Shortcut with zero projects starts a meeting; after about 20 lines it is filed into a new project; the card shows "Created …".
2. With existing projects, a meeting about one of them is filed into it and renamed.
3. Undo on the card returns the meeting to Unfiled, deletes the just-created empty project, and the assistant does not file it again.
4. Moving the meeting with the picker during a live meeting stops filing; the observer uses the new project's brief.
5. Assistant off: the meeting stays Unfiled and shows "Unfiled · pick a project".
6. A short meeting (under 5 lines) ended early stays Unfiled.
