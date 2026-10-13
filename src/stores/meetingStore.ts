import { create } from "zustand";
import { toast } from "sonner";
import i18n from "@/i18n";
import {
  commands,
  events,
  type Meeting,
  type MeetingDetail,
  type MeetingStatePayload,
} from "@/bindings";

interface MeetingStore {
  meetings: Meeting[];
  activeMeetingId: number | null;
  detail: MeetingDetail | null;
  live: MeetingStatePayload;
  loading: boolean;

  loadMeetings: (projectId?: number | null) => Promise<void>;
  openMeeting: (id: number) => Promise<void>;
  closeMeeting: () => void;
  startMeeting: (
    projectId: number | null,
    meetingId: number | null,
  ) => Promise<Meeting | null>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  end: () => Promise<void>;
  refreshLive: () => Promise<void>;
  renameMeeting: (id: number, title: string) => Promise<void>;
  saveContext: (id: number, context: string) => Promise<void>;
  deleteMeeting: (id: number) => Promise<void>;
  subscribe: () => Promise<() => void>;
}

const t = (key: string) => i18n.t(key);
const idle: MeetingStatePayload = {
  meeting_id: null,
  session_id: null,
  status: null,
};

export const useMeetingStore = create<MeetingStore>((set, get) => ({
  meetings: [],
  activeMeetingId: null,
  detail: null,
  live: idle,
  loading: false,

  loadMeetings: async (projectId = null) => {
    set({ loading: true });
    try {
      const res = await commands.listMeetings(projectId);
      if (res.status === "ok") set({ meetings: res.data });
      else toast.error(t("meetings.errors.load"));
    } finally {
      set({ loading: false });
    }
  },

  openMeeting: async (id) => {
    set({ activeMeetingId: id, detail: null });
    const res = await commands.getMeetingDetail(id);
    if (res.status === "ok" && get().activeMeetingId === id)
      set({ detail: res.data });
  },

  closeMeeting: () => set({ activeMeetingId: null, detail: null }),

  startMeeting: async (projectId, meetingId) => {
    const res = await commands.startMeeting(projectId, meetingId);
    if (res.status !== "ok") {
      toast.error(`${t("meetings.errors.start")} ${res.error}`);
      return null;
    }
    await get().refreshLive();
    await get().openMeeting(res.data.id);
    return res.data;
  },

  pause: async () => {
    const res = await commands.pauseMeeting();
    if (res.status !== "ok") toast.error(t("meetings.errors.control"));
    await get().refreshLive();
  },

  resume: async () => {
    const res = await commands.resumeMeeting();
    if (res.status !== "ok")
      toast.error(`${t("meetings.errors.control")} ${res.error}`);
    await get().refreshLive();
  },

  end: async () => {
    const res = await commands.endMeeting();
    if (res.status !== "ok") toast.error(t("meetings.errors.control"));
    await get().refreshLive();
    const id = get().activeMeetingId;
    if (id !== null) await get().openMeeting(id);
  },

  refreshLive: async () => {
    const live = await commands.meetingState();
    set({ live });
    const id = get().activeMeetingId;
    if (id !== null && live.meeting_id === id && live.status) {
      set((s) =>
        s.detail
          ? {
              detail: {
                ...s.detail,
                meeting: { ...s.detail.meeting, status: live.status! },
              },
            }
          : s,
      );
    }
  },

  renameMeeting: async (id, title) => {
    const res = await commands.updateMeetingTitle(id, title);
    if (res.status !== "ok") {
      toast.error(t("meetings.errors.save"));
      return;
    }
    set((s) => ({
      meetings: s.meetings.map((m) => (m.id === id ? { ...m, title } : m)),
      detail:
        s.detail && s.detail.meeting.id === id
          ? { ...s.detail, meeting: { ...s.detail.meeting, title } }
          : s.detail,
    }));
  },

  saveContext: async (id, context) => {
    const res = await commands.updateMeetingContext(id, context);
    if (res.status !== "ok") toast.error(t("meetings.errors.save"));
  },

  deleteMeeting: async (id) => {
    const res = await commands.deleteMeeting(id);
    if (res.status !== "ok") {
      toast.error(`${t("meetings.errors.delete")} ${res.error}`);
      return;
    }
    set((s) => ({
      meetings: s.meetings.filter((m) => m.id !== id),
      activeMeetingId: s.activeMeetingId === id ? null : s.activeMeetingId,
      detail: s.activeMeetingId === id ? null : s.detail,
    }));
  },

  subscribe: async () => {
    const un1 = await events.meetingStatePayload.listen((e) => {
      set({ live: e.payload });
      const id = get().activeMeetingId;
      if (id !== null && e.payload.meeting_id === id && e.payload.status) {
        set((s) =>
          s.detail
            ? {
                detail: {
                  ...s.detail,
                  meeting: { ...s.detail.meeting, status: e.payload.status! },
                },
              }
            : s,
        );
      }
    });
    const un2 = await events.transcriptLinePayload.listen((e) => {
      set((s) => {
        if (s.activeMeetingId !== e.payload.meeting_id || !s.detail) return s;
        if (s.detail.lines.some((l) => l.id === e.payload.line.id)) return s;
        return {
          detail: { ...s.detail, lines: [...s.detail.lines, e.payload.line] },
        };
      });
    });
    return () => {
      un1();
      un2();
    };
  },
}));
