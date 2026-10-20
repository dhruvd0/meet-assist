import { create } from "zustand";
import { commands, events, type CloudStatus } from "@/bindings";
import { useMeetingStore } from "./meetingStore";
import { useProjectStore } from "./projectStore";
import { useSettingsStore } from "./settingsStore";

interface CloudStore {
  status: CloudStatus | null;
  load: () => Promise<void>;
  /** Track sync status and reload local views when a pull changed data. */
  subscribe: () => Promise<() => void>;
  apply: (status: CloudStatus) => void;
}

export const useCloudStore = create<CloudStore>((set, get) => ({
  status: null,
  load: async () => {
    get().apply(await commands.cloudStatus());
  },
  apply: (status) => {
    const previous = get().status;
    set({ status });
    if (previous && status.revision !== previous.revision) {
      void useProjectStore.getState().loadProjects();
      void useMeetingStore.getState().loadMeetings(null);
      void useSettingsStore.getState().refreshSettings();
    }
  },
  subscribe: () => events.cloudStatus.listen((e) => get().apply(e.payload)),
}));
