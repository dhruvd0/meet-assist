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
  loading: boolean;

  loadProjects: () => Promise<void>;
  createProject: (title: string) => Promise<Project | null>;
  openProject: (id: number) => Promise<void>;
  closeProject: () => void;
  renameProject: (id: number, title: string) => Promise<void>;
  saveBrief: (id: number, brief: string) => Promise<void>;
  deleteProject: (id: number) => Promise<void>;
  loadSources: (projectId: number) => Promise<void>;
  addSource: (
    projectId: number,
    kind: SourceKind,
    title: string,
    locator: string,
  ) => Promise<Source | null>;
  removeSource: (id: number) => Promise<void>;
  reindexSource: (id: number) => Promise<void>;
  setCaption: (id: number, caption: string) => Promise<void>;
  subscribe: () => Promise<() => void>;
}

const t = (key: string) => i18n.t(key);

export const useProjectStore = create<ProjectStore>((set, get) => ({
  projects: [],
  activeProjectId: null,
  sources: [],
  loading: false,

  loadProjects: async () => {
    set({ loading: true });
    const res = await commands.listProjects();
    if (res.status === "ok") {
      set({ projects: res.data, loading: false });
    } else {
      set({ loading: false });
      toast.error(t("projects.errors.load"));
    }
  },

  createProject: async (title) => {
    const res = await commands.createProject(title);
    if (res.status !== "ok") {
      toast.error(t("projects.errors.create"));
      return null;
    }
    set((s) => ({ projects: [res.data, ...s.projects] }));
    return res.data;
  },

  openProject: async (id) => {
    set({ activeProjectId: id, sources: [] });
    await get().loadSources(id);
  },

  closeProject: () => set({ activeProjectId: null, sources: [] }),

  renameProject: async (id, title) => {
    const res = await commands.updateProjectTitle(id, title);
    if (res.status !== "ok") {
      toast.error(t("projects.errors.save"));
      return;
    }
    set((s) => ({
      projects: s.projects.map((p) => (p.id === id ? { ...p, title } : p)),
    }));
  },

  saveBrief: async (id, brief) => {
    const res = await commands.updateProjectBrief(id, brief);
    if (res.status !== "ok") {
      toast.error(t("projects.errors.save"));
      return;
    }
    set((s) => ({
      projects: s.projects.map((p) => (p.id === id ? { ...p, brief } : p)),
    }));
  },

  deleteProject: async (id) => {
    const res = await commands.deleteProject(id);
    if (res.status !== "ok") {
      toast.error(t("projects.errors.delete"));
      return;
    }
    set((s) => ({
      projects: s.projects.filter((p) => p.id !== id),
      activeProjectId: s.activeProjectId === id ? null : s.activeProjectId,
      sources: s.activeProjectId === id ? [] : s.sources,
    }));
  },

  loadSources: async (projectId) => {
    const res = await commands.listSources(projectId);
    if (res.status === "ok" && get().activeProjectId === projectId) {
      set({ sources: res.data });
    }
  },

  addSource: async (projectId, kind, title, locator) => {
    const res = await commands.addSource(projectId, kind, title, locator);
    if (res.status !== "ok") {
      toast.error(t("projects.sources.errors.add"));
      return null;
    }
    set((s) => {
      if (s.activeProjectId !== res.data.project_id) return s;
      const exists = s.sources.some((x) => x.id === res.data.id);
      return {
        sources: exists
          ? s.sources.map((x) => (x.id === res.data.id ? res.data : x))
          : [...s.sources, res.data],
      };
    });
    return res.data;
  },

  removeSource: async (id) => {
    const res = await commands.removeSource(id);
    if (res.status !== "ok") {
      toast.error(t("projects.sources.errors.remove"));
      return;
    }
    set((s) => ({ sources: s.sources.filter((x) => x.id !== id) }));
  },

  reindexSource: async (id) => {
    await commands.reindexSource(id);
  },

  setCaption: async (id, caption) => {
    const res = await commands.setSourceCaption(id, caption);
    if (res.status !== "ok") {
      toast.error(t("projects.errors.save"));
      return;
    }
    set((s) => ({
      sources: s.sources.map((x) =>
        x.id === id ? { ...x, digest: caption } : x,
      ),
    }));
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
