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
import { useProjectStore } from "./projectStore";

export type ObserverReadinessCode =
  | "ready"
  | "disabled"
  | "no_provider"
  | "unsupported_provider"
  | "missing_api_key"
  | "no_model";

interface ObserverView {
  tools_unavailable: boolean;
  consecutive_failures: number;
  last_error: string | null;
  readiness: ObserverReadinessCode;
  detail: string | null;
}

interface MeetingStore {
  meetings: Meeting[];
  activeMeetingId: number | null;
  detail: MeetingDetail | null;
  live: MeetingStatePayload;
  observer: ObserverView;
  loading: boolean;
  captureWarning: {
    meeting_id: number;
    source: string;
    message: string;
  } | null;

  loadMeetings: (projectId?: number | null) => Promise<void>;
  openMeeting: (id: number) => Promise<void>;
  closeMeeting: () => void;
  startMeeting: (
    projectId: number | null,
    meetingId: number | null,
  ) => Promise<Meeting | null>;
  pause: () => Promise<void>;
  resume: () => Promise<void>;
  end: (meetingId: number | null) => Promise<void>;
  refreshLive: () => Promise<void>;
  renameMeeting: (id: number, title: string) => Promise<void>;
  moveMeeting: (id: number, projectId: number | null) => Promise<void>;
  saveContext: (id: number, context: string) => Promise<void>;
  deleteMeeting: (id: number) => Promise<void>;
  dismiss: (id: number) => Promise<void>;
  summarize: (sessionId: number) => Promise<void>;
  subscribe: () => Promise<() => void>;
}

const t = (key: string) => i18n.t(key);
const idleCounters = {
  tools_unavailable: false,
  consecutive_failures: 0,
  last_error: null,
};
const idleObserver: ObserverView = {
  ...idleCounters,
  readiness: "ready",
  detail: null,
};
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
  observer: idleObserver,
  loading: false,
  captureWarning: null,

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
    set((s) => ({
      activeMeetingId: id,
      detail: null,
      captureWarning:
        s.captureWarning?.meeting_id === id ? s.captureWarning : null,
      observer: { ...s.observer, ...idleCounters },
    }));
    const res = await commands.getMeetingDetail(id);
    if (res.status !== "ok") {
      toast.error(`${t("meetings.errors.load")} ${res.error}`);
      return;
    }
    if (get().activeMeetingId === id) set({ detail: res.data });
  },

  closeMeeting: () =>
    set((s) => ({
      activeMeetingId: null,
      detail: null,
      captureWarning: null,
      observer: { ...s.observer, ...idleCounters },
    })),

  startMeeting: async (projectId, meetingId) => {
    set({ captureWarning: null });
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
    if (res.status !== "ok")
      toast.error(`${t("meetings.errors.control")} ${res.error}`);
    await get().refreshLive();
  },

  resume: async () => {
    const res = await commands.resumeMeeting();
    if (res.status !== "ok")
      toast.error(`${t("meetings.errors.control")} ${res.error}`);
    await get().refreshLive();
  },

  end: async (meetingId) => {
    const res = await commands.endMeeting(meetingId);
    if (res.status !== "ok")
      toast.error(`${t("meetings.errors.control")} ${res.error}`);
    await get().refreshLive();
    const id = get().activeMeetingId;
    if (id !== null) await get().openMeeting(id);
  },

  refreshLive: async () => {
    const [live, ready] = await Promise.all([
      commands.meetingState(),
      commands.getObserverReadiness(),
    ]);
    set((s) => ({
      live,
      observer: {
        ...s.observer,
        readiness: ready.readiness as ObserverReadinessCode,
        detail: ready.detail,
      },
    }));
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

  moveMeeting: async (id, projectId) => {
    const res = await commands.moveMeeting(id, projectId);
    if (res.status !== "ok")
      toast.error(`${t("meetings.errors.save")} ${res.error}`);
    // State updates arrive through the meetingFiledPayload event.
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

  dismiss: async (id) => {
    const res = await commands.dismissIntervention(id);
    if (res.status !== "ok") {
      toast.error(t("meetings.errors.save"));
      return;
    }
    set((s) =>
      s.detail
        ? {
            detail: {
              ...s.detail,
              interventions: s.detail.interventions.map((iv) =>
                iv.id === id ? { ...iv, dismissed: true } : iv,
              ),
            },
          }
        : s,
    );
  },

  summarize: async (sessionId) => {
    const res = await commands.summarizeSession(sessionId);
    if (res.status !== "ok")
      toast.error(`${t("meetings.summary.failed")} ${res.error}`);
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
    const un3 = await events.meetingCaptureWarningPayload.listen((e) => {
      const { source, message, meeting_id } = e.payload;
      if (source === "system_audio") {
        // Shown as a banner in the open meeting instead of a toast.
        set({ captureWarning: { meeting_id, source, message } });
        return;
      }
      toast.error(`${t("meetings.errors.capture")} ${message}`);
    });
    const un4 = await events.interventionPayload.listen((e) => {
      set((s) => {
        if (s.activeMeetingId !== e.payload.meeting_id || !s.detail) return s;
        if (
          s.detail.interventions.some((i) => i.id === e.payload.intervention.id)
        )
          return s;
        return {
          detail: {
            ...s.detail,
            interventions: [...s.detail.interventions, e.payload.intervention],
          },
        };
      });
    });
    const un5 = await events.observerStatusPayload.listen((e) => {
      const {
        meeting_id,
        tools_unavailable,
        consecutive_failures,
        last_error,
        readiness,
        detail,
      } = e.payload;
      // Readiness is global; the counters belong to one meeting.
      const counters =
        meeting_id === null || meeting_id === get().activeMeetingId
          ? { tools_unavailable, consecutive_failures, last_error }
          : {};
      set((s) => ({
        observer: {
          ...s.observer,
          ...counters,
          readiness: readiness as ObserverReadinessCode,
          detail,
        },
      }));
    });
    const un6 = await events.openMeetingPayload.listen((e) => {
      get().openMeeting(e.payload.meeting_id);
    });
    const un7 = await events.sessionSummaryPayload.listen((e) => {
      const { meeting_id, session_id, summary, rolling_summary } = e.payload;
      set((s) => ({
        meetings: s.meetings.map((m) =>
          m.id === meeting_id ? { ...m, rolling_summary } : m,
        ),
        detail:
          s.detail && s.detail.meeting.id === meeting_id
            ? {
                ...s.detail,
                meeting: { ...s.detail.meeting, rolling_summary },
                sessions: s.detail.sessions.map((x) =>
                  x.id === session_id
                    ? { ...x, summary: JSON.stringify(summary) }
                    : x,
                ),
              }
            : s.detail,
      }));
    });
    const un8 = await events.meetingFiledPayload.listen((e) => {
      const { meeting_id, project_id, meeting_title, by } = e.payload;
      const patch = { project_id, title: meeting_title, filed_by: by };
      set((s) => ({
        meetings: s.meetings.map((m) =>
          m.id === meeting_id ? { ...m, ...patch } : m,
        ),
        detail:
          s.detail && s.detail.meeting.id === meeting_id
            ? { ...s.detail, meeting: { ...s.detail.meeting, ...patch } }
            : s.detail,
      }));
      // A project may have been created or deleted.
      useProjectStore.getState().loadProjects();
    });
    return () => {
      un7();
      un8();
      un1();
      un2();
      un3();
      un4();
      un5();
      un6();
    };
  },
}));
