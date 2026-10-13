import React, { useEffect } from "react";
import { useMeetingStore } from "@/stores/meetingStore";
import { MeetingList } from "./MeetingList";
import { MeetingDetail } from "./MeetingDetail";

export const MeetingsSection: React.FC = () => {
  const activeMeetingId = useMeetingStore((s) => s.activeMeetingId);
  const loadMeetings = useMeetingStore((s) => s.loadMeetings);
  const refreshLive = useMeetingStore((s) => s.refreshLive);
  const subscribe = useMeetingStore((s) => s.subscribe);
  const closeMeeting = useMeetingStore((s) => s.closeMeeting);
  const openMeeting = useMeetingStore((s) => s.openMeeting);

  useEffect(() => {
    loadMeetings(null);
    refreshLive();
    const id = useMeetingStore.getState().activeMeetingId;
    if (id !== null) openMeeting(id);
    const unlisten = subscribe();
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [loadMeetings, refreshLive, subscribe, openMeeting]);

  return activeMeetingId === null ? (
    <MeetingList />
  ) : (
    <MeetingDetail
      onBack={() => {
        closeMeeting();
        loadMeetings(null);
      }}
    />
  );
};
