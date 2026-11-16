-- Recording backup. One private folder per user in the meeting-audio bucket:
-- <user id>/<session id>.ogg is the session's recording and
-- <user id>/<session id>.json says where each transcript line sits in it.
-- The app writes the .json last, so its presence means the upload finished.

insert into storage.buckets (id, name, public, file_size_limit, allowed_mime_types)
values ('meeting-audio', 'meeting-audio', false, 52428800, array['audio/ogg', 'application/json'])
on conflict (id) do nothing;

create policy "own recordings" on storage.objects
  for all to authenticated
  using (bucket_id = 'meeting-audio' and (storage.foldername(name))[1] = (select auth.uid())::text)
  with check (bucket_id = 'meeting-audio' and (storage.foldername(name))[1] = (select auth.uid())::text);
