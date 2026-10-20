# Cloud sync and accounts

CASE stays offline-first. The local SQLite database and the local settings store remain the source of truth on every device. Signing in to a CASE account adds a background process that copies projects, meetings, sessions, and portable settings to Supabase, and pulls changes made on the user's other devices. Signed out, nothing changes and nothing leaves the machine.

## Goals

- Basic accounts: email and password sign-up, sign-in, sign-out. Sign-up confirms immediately (email confirmation is off).
- Every write keeps going to SQLite first. The UI never waits on the network.
- A background loop pushes local changes and pulls remote ones while signed in. Offline is a normal state, not an error.
- A second device signed in to the same account converges to the same projects, meetings, sessions, transcript lines, interventions, and portable settings.

## Non-goals

- Real-time sync, collaborative editing, or sharing between accounts.
- Syncing API keys, audio, source chunks (the full-text index), observer internals, or device-specific settings.
- Merging two accounts' data on one device.

## Backend

Supabase project `case` (ref `fjohdmmgmvcjdoswkvsq`, region ap-northeast-2), schema in `supabase/migrations/20261021000000_cloud_sync.sql`.

- URL `https://fjohdmmgmvcjdoswkvsq.supabase.co`, publishable key `sb_publishable_81VLvYMSAX1a8yPqhQzsaA_bqP4VJob`. Both are public by design; access control is row-level security.
- Tables `projects`, `sources`, `meetings`, `sessions`, `transcript_lines`, `interventions`: `id uuid` (client-generated), `user_id` (defaults to `auth.uid()`), `updated_at bigint` (client ms), `deleted boolean` (tombstone), `synced_at timestamptz` (server-stamped pull cursor), plus the content columns. Parent links are uuids (`project_id`, `meeting_id`, `session_id`), with no remote foreign keys so rows can arrive in any order.
- `user_settings(user_id pk, updated_at bigint, data jsonb, synced_at)`.
- A trigger stamps `synced_at = now()` on every write and drops an update whose `updated_at` is older than the stored row. That makes the server last-writer-wins and makes every push idempotent.
- RLS limits every operation to `user_id = auth.uid()`. `anon` has no access. `authenticated` has select, insert, update. There is no delete grant, so deletes are tombstones.

## Local tracking (SQLite migration)

- Add `uid TEXT` with a unique index to the six synced tables, backfilled with lowercase UUID v4 strings. An `AFTER INSERT` trigger fills `uid` when the app inserts a row without one, so no store call site changes.
- `sync_outbox(entity TEXT, uid TEXT, deleted INTEGER, changed_at INTEGER, version INTEGER, PRIMARY KEY(entity, uid))`. `AFTER INSERT/UPDATE/DELETE` triggers on each synced table upsert a row here, bumping `version` and setting `changed_at` to the current ms. The migration seeds the outbox with every existing row, so the first sign-in uploads history.
- `sync_guard(applying INTEGER)` single row. Every outbox trigger has `WHEN (SELECT applying FROM sync_guard) = 0`. Pull-apply runs in one `BEGIN IMMEDIATE` transaction that sets `applying = 1`, writes, and resets it, so applied remote rows never echo back into the outbox.
- `sync_cursor(entity PRIMARY KEY, synced_at TEXT)` and `sync_meta(key PRIMARY KEY, value)` for the owner account and settings sync state.

Triggers own change capture because every store function already funnels through SQL. No store method needs a sync call, and cascaded deletes (meeting to sessions to lines) are captured for free because SQLite fires triggers on foreign-key actions.

## Sync loop

One background task, started at app launch, runs a cycle every 30 seconds and immediately after sign-in or "Sync now". A cycle does nothing when signed out.

1. **Refresh** the access token if it expires within 60 seconds. A failed refresh with an auth error signs the user out locally and reports it. A network failure reports `offline` and ends the cycle.
2. **Push** the outbox in parent-first order: projects, sources, meetings, sessions, transcript_lines, interventions. Batches of up to 500 rows go to PostgREST as an upsert (`Prefer: resolution=merge-duplicates`). Each row's `updated_at` is its outbox `changed_at`, and local parent ids are mapped to parent uids. A deleted row is sent as `{id, deleted: true, updated_at}`. After a successful batch, delete outbox rows whose `version` still matches what was read, so a change made mid-push is pushed next cycle.
3. **Pull** each table in the same order, filtering `synced_at >= cursor - 10s`, ordered by `synced_at, id`, 1000 rows per page. The overlap re-reads rows that committed out of order; applying them twice is harmless. Apply rules:
   - A row whose uid is in the local outbox is skipped. The local edit is newer than the last push and wins.
   - `deleted = true` deletes the local row by uid.
   - Otherwise upsert by uid, mapping parent uids to local ids. A row whose parent is missing locally is skipped and retried next cycle by not advancing that table's cursor past it.
   - Pulled sources are inserted with `status = 'pending'` so the indexer rebuilds their chunks. A file or folder path from another machine fails indexing and shows its error, which is expected.
4. **Settings.** The portable subset is serialized to JSON and hashed. If the local hash differs from the last synced hash, push it with `updated_at = now`. Otherwise, if the remote `updated_at` is newer than the last applied remote value, apply the remote subset through the normal settings write path. Local unsynced changes win.

Portable settings: `audio_feedback`, `audio_feedback_volume`, `sound_theme`, `translate_to_english`, `selected_language`, `custom_words`, `word_correction_threshold`, `llm_provider_id`, `llm_providers`, `llm_models`, `meeting_assistant_enabled`, `meeting_system_audio_enabled`, `meeting_vision_enabled`, `meeting_jev_enabled`, `meeting_sensitivity`, `meeting_observer_interval_secs`, `meeting_max_line_secs`, `meeting_beep_enabled`, `app_language`, `theme`, `filler_word_removal_enabled`, `custom_filler_words`, `chinese_script`. Everything else stays local: `llm_api_keys`, devices, shortcuts, model choice, accelerators, onboarding, debug, autostart, tray, updater.

## Accounts and secrets

- Auth calls go to Supabase Auth (`/auth/v1/signup`, `/auth/v1/token?grant_type=password`, `/auth/v1/token?grant_type=refresh_token`, `/auth/v1/logout`) through `reqwest`. There is no second HTTP client.
- The session (access token, refresh token, expiry, user id, email) is stored in the OS keychain through the `keyring` crate, never in the settings store or SQLite.
- The local database binds to the first account that signs in (`sync_meta.owner_user_id`). Signing in with a different account on the same device is accepted, but sync stays off with an `account_mismatch` status until the user signs back in to the owner account. This prevents one account's meetings from uploading into another.
- Signing out deletes the keychain entry and stops the loop. Local data stays.

## Status and UI

- Rust emits `cloud-status` with `{ state: signed_out | idle | syncing | offline | error | account_mismatch, email, last_synced_at, pending, message }`.
- Commands: `cloud_status`, `cloud_sign_up`, `cloud_sign_in`, `cloud_sign_out`, `cloud_sync_now`.
- A new Account tab in Settings. Signed out, it shows email and password with Sign in and Create account, plus one sentence that sync is optional and CASE works fully without it. Signed in, it shows the email, the sync state, last synced time, pending count, Sync now, and Sign out. All strings are `account.*` i18n keys.

## Files

- `src-tauri/src/cloud/mod.rs` owns constants and the status type. `auth.rs` owns sessions, keychain, and auth calls. `sync.rs` owns the loop, push, pull, and settings sync.
- `src-tauri/src/meeting/store.rs` gains the migration plus a small sync API: read outbox batches, ack, apply remote rows under the guard, and cursors.
- `src-tauri/src/commands/cloud.rs` holds the commands. `src/components/settings/account/` holds the tab.

## Tests

Store tests, inside the existing data-safety core:

- Inserting, updating, and deleting a project records outbox rows, and the cascade from deleting a meeting records its sessions and lines.
- Applying remote rows under the guard records nothing in the outbox.
- Acking with a stale version keeps the outbox row.
- The migration backfills a uid for existing rows and seeds the outbox.

End to end: run the sync cycle against the live project with a throwaway account. Push from one database, pull into a second empty database, and compare.
