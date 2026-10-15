import { create } from "zustand";
import { toast } from "sonner";
import i18n from "@/i18n";
import {
  commands,
  events,
  type Meeting,
  type MeetingDetail,
  type MeetingStatePayload,
  type ObserverThought,
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
  thoughts: ObserverThought[];
  readiness: ObserverReadinessCode;
  detail: string | null;
}

/** A start/pause/resume/end request still waiting on the backend. */
export type MeetingControl = "start" | "pause" | "resume" | "end";

/** A session summary that is not stored yet. */
export type SummaryProgress =
  | { status: "generating" }
  | { status: "failed"; error: string };

interface MeetingStore {
  meetings: Meeting[];
  activeMeetingId: number | null;
  detail: MeetingDetail | null;
  /** Why the open meeting could not be shown, when it could not. */
  detailError: string | null;
  controlPending: MeetingControl | null;
  /** The last start/pause/resume/end failure, for surfaces without toasts
   * (the floating bar). Cleared when the next control starts. */
  controlError: string | null;
  /** Why the meeting list failed to load, when it did. */
  meetingsError: string | null;
  /** Per session id; cleared once the summary is stored. */
  summaryProgress: Record<number, SummaryProgress>;
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
  /** Resolves once the summary is stored or has failed (see summaryProgress). */
  summarize: (sessionId: number) => Promise<void>;
  subscribe: () => Promise<() => void>;
}

const t = (key: string) => i18n.t(key);
const idleCounters = {
  tools_unavailable: false,
  consecutive_failures: 0,
  last_error: null,
  thoughts: [] as ObserverThought[],
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

type Set = (
  partial: Partial<MeetingStore> | ((s: MeetingStore) => Partial<MeetingStore>),
) => void;

const setProgress = (
  set: Set,
  sessionId: number,
  progress: SummaryProgress | null,
) =>
  set((s) => {
    const next = { ...s.summaryProgress };
    if (progress) next[sessionId] = progress;
    else delete next[sessionId];
    return { summaryProgress: next };
  });

const controlFailed = (set: Set, message: string) => {
  toast.error(message);
  set({ controlError: message });
};

/** Run one meeting control at a time, marking it pending while it runs. */
const control = async (
  set: Set,
  get: () => MeetingStore,
  kind: MeetingControl,
  run: () => Promise<void>,
) => {
  if (get().controlPending) return;
  set({ controlPending: kind, controlError: null });
  try {
    await run();
  } finally {
    set({ controlPending: null });
  }
};

export const useMeetingStore = create<MeetingStore>((set, get) => ({
  meetings: [],
  activeMeetingId: null,
  detail: null,
  detailError: null,
  controlPending: null,
  controlError: null,
  meetingsError: null,
  summaryProgress: {},
  live: idle,
  observer: idleObserver,
  loading: false,
  captureWarning: null,

  loadMeetings: async (projectId = null) => {
    set({ loading: true, meetingsError: null });
    try {
      const res = await commands.listMeetings(projectId);
      if (res.status === "ok") set({ meetings: res.data });
      else set({ meetingsError: `${t("meetings.errors.load")} ${res.error}` });
    } finally {
      set({ loading: false });
    }
  },

  openMeeting: async (id) => {
    set((s) => ({
      activeMeetingId: id,
      detail: null,
      detailError: null,
      captureWarning:
        s.captureWarning?.meeting_id === id ? s.captureWarning : null,
      observer: { ...s.observer, ...idleCounters },
    }));
    const res = await commands.getMeetingDetail(id);
    if (get().activeMeetingId !== id) return;
    if (res.status !== "ok") {
      set({ detailError: `${t("meetings.errors.load")} ${res.error}` });
      return;
    }
    if (!res.data) {
      set({ detailError: t("meetings.errors.notFound") });
      return;
    }
    set({ detail: res.data });
  },

  closeMeeting: () =>
    set((s) => ({
      activeMeetingId: null,
      detail: null,
      detailError: null,
      captureWarning: null,
      observer: { ...s.observer, ...idleCounters },
    })),

  startMeeting: async (projectId, meetingId) => {
    if (get().controlPending) return null;
    set({ captureWarning: null, controlPending: "start", controlError: null });
    try {
      const res = await commands.startMeeting(projectId, meetingId);
      if (res.status !== "ok") {
        controlFailed(set, `${t("meetings.errors.start")} ${res.error}`);
        return null;
      }
      await get().refreshLive();
      await get().openMeeting(res.data.id);
      return res.data;
    } finally {
      set({ controlPending: null });
    }
  },

  pause: () =>
    control(set, get, "pause", async () => {
      const res = await commands.pauseMeeting();
      if (res.status !== "ok")
        controlFailed(set, `${t("meetings.errors.control")} ${res.error}`);
      await get().refreshLive();
    }),

  resume: () =>
    control(set, get, "resume", async () => {
      const res = await commands.resumeMeeting();
      if (res.status !== "ok")
        controlFailed(set, `${t("meetings.errors.control")} ${res.error}`);
      await get().refreshLive();
    }),

  end: (meetingId) =>
    control(set, get, "end", async () => {
      const res = await commands.endMeeting(meetingId);
      if (res.status !== "ok")
        controlFailed(set, `${t("meetings.errors.control")} ${res.error}`);
      await get().refreshLive();
      const id = get().activeMeetingId;
      if (id !== null) await get().openMeeting(id);
    }),

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
      toast.error(`${t("meetings.errors.save")} ${res.error}`);
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
    if (res.status !== "ok")
      toast.error(`${t("meetings.errors.save")} ${res.error}`);
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
      toast.error(`${t("meetings.errors.save")} ${res.error}`);
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
    setProgress(set, sessionId, { status: "generating" });
    const res = await commands.summarizeSession(sessionId);
    // Success clears the progress through the summary event.
    if (res.status !== "ok")
      setProgress(set, sessionId, { status: "failed", error: res.error });
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
        thoughts,
      } = e.payload;
      // Readiness is global; the counters belong to one meeting.
      const counters =
        meeting_id === null || meeting_id === get().activeMeetingId
          ? { tools_unavailable, consecutive_failures, last_error, thoughts }
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
    const un9 = await events.sessionSummaryStatusPayload.listen((e) => {
      const { session_id, status, error } = e.payload;
      setProgress(
        set,
        session_id,
        status === "failed"
          ? { status: "failed", error: error ?? "" }
          : { status: "generating" },
      );
    });
    const un7 = await events.sessionSummaryPayload.listen((e) => {
      const { meeting_id, session_id, summary, rolling_summary } = e.payload;
      setProgress(set, session_id, null);
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
    const un10 = await events.meetingTitledPayload.listen((e) => {
      const { meeting_id, title } = e.payload;
      set((s) => ({
        meetings: s.meetings.map((m) =>
          m.id === meeting_id ? { ...m, title } : m,
        ),
        detail:
          s.detail && s.detail.meeting.id === meeting_id
            ? { ...s.detail, meeting: { ...s.detail.meeting, title } }
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
      un10();
      un9();
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
