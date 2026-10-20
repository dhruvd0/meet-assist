-- CASE cloud sync. SQLite on each device is the source of truth; these tables
-- are a per-user replica. Rows are keyed by a client-generated uuid, written
-- last-writer-wins on the client's updated_at (ms), and never hard-deleted:
-- a delete is a tombstone (deleted = true) so other devices can apply it.
-- synced_at is the server's pull cursor; clients re-read a short overlap
-- window, so out-of-order commits are not skipped.

create or replace function public.case_sync_stamp() returns trigger
language plpgsql set search_path = '' as $$
begin
  if tg_op = 'UPDATE' then
    if new.updated_at < old.updated_at then
      return old;
    end if;
    new.user_id := old.user_id;
  end if;
  new.synced_at := now();
  return new;
end;
$$;

create table public.projects (
  id uuid primary key,
  user_id uuid not null default auth.uid() references auth.users(id) on delete cascade,
  updated_at bigint not null,
  deleted boolean not null default false,
  synced_at timestamptz not null default now(),
  title text,
  brief text,
  created_at bigint
);

create table public.sources (
  id uuid primary key,
  user_id uuid not null default auth.uid() references auth.users(id) on delete cascade,
  updated_at bigint not null,
  deleted boolean not null default false,
  synced_at timestamptz not null default now(),
  project_id uuid,
  kind text,
  title text,
  locator text,
  added_at bigint,
  digest text
);

create table public.meetings (
  id uuid primary key,
  user_id uuid not null default auth.uid() references auth.users(id) on delete cascade,
  updated_at bigint not null,
  deleted boolean not null default false,
  synced_at timestamptz not null default now(),
  project_id uuid,
  title text,
  created_at bigint,
  status text,
  context text,
  rolling_summary text,
  filed_by text,
  input_mode text
);

create table public.sessions (
  id uuid primary key,
  user_id uuid not null default auth.uid() references auth.users(id) on delete cascade,
  updated_at bigint not null,
  deleted boolean not null default false,
  synced_at timestamptz not null default now(),
  meeting_id uuid,
  started_at bigint,
  ended_at bigint,
  summary text
);

create table public.transcript_lines (
  id uuid primary key,
  user_id uuid not null default auth.uid() references auth.users(id) on delete cascade,
  updated_at bigint not null,
  deleted boolean not null default false,
  synced_at timestamptz not null default now(),
  session_id uuid,
  ts bigint,
  speaker text,
  text text
);

create table public.interventions (
  id uuid primary key,
  user_id uuid not null default auth.uid() references auth.users(id) on delete cascade,
  updated_at bigint not null,
  deleted boolean not null default false,
  synced_at timestamptz not null default now(),
  session_id uuid,
  ts bigint,
  kind text,
  text text,
  reason text,
  dismissed boolean
);

create table public.user_settings (
  user_id uuid primary key default auth.uid() references auth.users(id) on delete cascade,
  updated_at bigint not null,
  synced_at timestamptz not null default now(),
  data jsonb not null default '{}'::jsonb
);

do $$
declare t text;
begin
  foreach t in array array['projects','sources','meetings','sessions','transcript_lines','interventions','user_settings'] loop
    execute format('alter table public.%I enable row level security', t);
    execute format('create policy "own rows" on public.%I for all to authenticated using (user_id = (select auth.uid())) with check (user_id = (select auth.uid()))', t);
    execute format('create trigger %I before insert or update on public.%I for each row execute function public.case_sync_stamp()', t || '_sync_stamp', t);
    execute format('revoke all on public.%I from anon, authenticated', t);
    execute format('grant select, insert, update on public.%I to authenticated', t);
  end loop;
  foreach t in array array['projects','sources','meetings','sessions','transcript_lines','interventions'] loop
    execute format('create index %I on public.%I (user_id, synced_at, id)', t || '_pull_idx', t);
  end loop;
end;
$$;
