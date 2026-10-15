import { create } from "zustand";
import { toast } from "sonner";
import i18n from "@/i18n";
import {
  commands,
  events,
  type Project,
  type Source,
  type SourceKind,
} from "@/bindings";

interface ProjectStore {
  projects: Project[];
  activeProjectId: number | null;
  sources: Source[];
  /** Loading state of `sources` for the open project. */
  sourcesState: "loading" | "ready" | "error";
  sourcesError: string | null;
  loading: boolean;
  /** Why `projects` failed to load, when it did. */
  projectsError: string | null;

  loadProjects: () => Promise<void>;
  createProject: (title: string) => Promise<Project | null>;
  openProject: (id: number) => Promise<void>;
  closeProject: () => void;
  renameProject: (id: number, title: string) => Promise<void>;
  saveBrief: (id: number, brief: string) => Promise<void>;
  /** True when the project was deleted. */
  deleteProject: (id: number) => Promise<boolean>;
  loadSources: (projectId: number) => Promise<void>;
  addSource: (
    projectId: number,
    kind: SourceKind,
    title: string,
    locator: string,
  ) => Promise<Source | null>;
  removeSource: (id: number) => Promise<void>;
  reindexSource: (id: number) => Promise<void>;
  /** True when the caption was saved. */
  setCaption: (id: number, caption: string) => Promise<boolean>;
  subscribe: () => Promise<() => void>;
}

const t = (key: string) => i18n.t(key);

export const useProjectStore = create<ProjectStore>((set, get) => ({
  projects: [],
  activeProjectId: null,
  sources: [],
  sourcesState: "loading",
  sourcesError: null,
  loading: false,
  projectsError: null,

  loadProjects: async () => {
    set({ loading: true, projectsError: null });
    try {
      const res = await commands.listProjects();
      if (res.status === "ok") {
        set({ projects: res.data });
      } else {
        set({ projectsError: `${t("projects.errors.load")} ${res.error}` });
        toast.error(`${t("projects.errors.load")} ${res.error}`);
      }
    } finally {
      set({ loading: false });
    }
  },

  createProject: async (title) => {
    const res = await commands.createProject(title);
    if (res.status !== "ok") {
      toast.error(`${t("projects.errors.create")} ${res.error}`);
      return null;
    }
    set((s) => ({ projects: [res.data, ...s.projects] }));
    return res.data;
  },

  openProject: async (id) => {
    set({
      activeProjectId: id,
      sources: [],
      sourcesState: "loading",
      sourcesError: null,
    });
    await get().loadSources(id);
  },

  closeProject: () => set({ activeProjectId: null, sources: [] }),

  renameProject: async (id, title) => {
    const res = await commands.updateProjectTitle(id, title);
    if (res.status !== "ok") {
      toast.error(`${t("projects.errors.save")} ${res.error}`);
      return;
    }
    set((s) => ({
      projects: s.projects.map((p) => (p.id === id ? { ...p, title } : p)),
    }));
  },

  saveBrief: async (id, brief) => {
    const res = await commands.updateProjectBrief(id, brief);
    if (res.status !== "ok") {
      toast.error(`${t("projects.errors.save")} ${res.error}`);
      return;
    }
    set((s) => ({
      projects: s.projects.map((p) => (p.id === id ? { ...p, brief } : p)),
    }));
  },

  deleteProject: async (id) => {
    const res = await commands.deleteProject(id);
    if (res.status !== "ok") {
      toast.error(`${t("projects.errors.delete")} ${res.error}`);
      return false;
    }
    set((s) => ({
      projects: s.projects.filter((p) => p.id !== id),
      activeProjectId: s.activeProjectId === id ? null : s.activeProjectId,
      sources: s.activeProjectId === id ? [] : s.sources,
    }));
    return true;
  },

  loadSources: async (projectId) => {
    if (get().activeProjectId === projectId)
      set({ sourcesState: "loading", sourcesError: null });
    const res = await commands.listSources(projectId);
    if (get().activeProjectId !== projectId) return;
    if (res.status === "ok") set({ sources: res.data, sourcesState: "ready" });
    else
      set({
        sourcesState: "error",
        sourcesError: `${t("projects.sources.errors.load")} ${res.error}`,
      });
  },

  addSource: async (projectId, kind, title, locator) => {
    const res = await commands.addSource(projectId, kind, title, locator);
    if (res.status !== "ok") {
      toast.error(`${t("projects.sources.errors.add")} ${res.error}`);
      return null;
    }
    set((s) => {
      if (s.activeProjectId !== res.data.project_id) return s;
      if (s.sources.some((x) => x.id === res.data.id)) return s;
      return { sources: [...s.sources, res.data] };
    });
    return res.data;
  },

  removeSource: async (id) => {
    const res = await commands.removeSource(id);
    if (res.status !== "ok") {
      toast.error(`${t("projects.sources.errors.remove")} ${res.error}`);
      return;
    }
    set((s) => ({ sources: s.sources.filter((x) => x.id !== id) }));
  },

  reindexSource: async (id) => {
    const res = await commands.reindexSource(id);
    if (res.status !== "ok") {
      toast.error(`${t("projects.sources.errors.reindex")} ${res.error}`);
    }
  },

  setCaption: async (id, caption) => {
    const res = await commands.setSourceCaption(id, caption);
    if (res.status !== "ok") {
      toast.error(`${t("projects.errors.save")} ${res.error}`);
      return false;
    }
    set((s) => ({
      sources: s.sources.map((x) =>
        x.id === id ? { ...x, digest: caption } : x,
      ),
    }));
    return true;
  },

  subscribe: async () => {
    const unlisten = await events.sourceStatusPayload.listen((event) => {
      const updated = event.payload.source;
      set((s) => {
        if (s.activeProjectId !== updated.project_id) return s;
        const exists = s.sources.some((x) => x.id === updated.id);
        return {
          sources: exists
            ? s.sources.map((x) => (x.id === updated.id ? updated : x))
            : [...s.sources, updated],
        };
      });
    });
    return unlisten;
  },
}));
