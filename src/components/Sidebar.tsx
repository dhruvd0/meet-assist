import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getVersion } from "@tauri-apps/api/app";
import { Folder, House, Plus, Settings } from "lucide-react";
import { useProjectStore } from "@/stores/projectStore";
import { useMeetingStore } from "@/stores/meetingStore";
import { Wordmark } from "./icons/Wordmark";
import ModelSelector from "./model-selector";
import UpdateChecker from "./update-checker";
import {
  AssistantSettings,
  AudioSettings,
  GeneralSettings,
  AccountSettings,
  AdvancedSettings,
  DebugSettings,
  AboutSettings,
  ModelsSettings,
} from "./settings";
import type { AppSettings } from "@/bindings";

interface SectionConfig {
  labelKey: string;
  component: React.ComponentType;
  enabled: (settings: AppSettings | null | undefined) => boolean;
}

/** Settings pages, shown as tabs inside the Settings view. */
export const SETTINGS_SECTIONS = {
  assistant: {
    labelKey: "sidebar.assistant",
    component: AssistantSettings,
    enabled: () => true,
  },
  audio: {
    labelKey: "sidebar.audio",
    component: AudioSettings,
    enabled: () => true,
  },
  models: {
    labelKey: "sidebar.models",
    component: ModelsSettings,
    enabled: () => true,
  },
  general: {
    labelKey: "sidebar.general",
    component: GeneralSettings,
    enabled: () => true,
  },
  account: {
    labelKey: "sidebar.account",
    component: AccountSettings,
    enabled: () => true,
  },
  advanced: {
    labelKey: "sidebar.advanced",
    component: AdvancedSettings,
    enabled: () => true,
  },
  debug: {
    labelKey: "sidebar.debug",
    component: DebugSettings,
    enabled: (settings) => settings?.debug_mode ?? false,
  },
  about: {
    labelKey: "sidebar.about",
    component: AboutSettings,
    enabled: () => true,
  },
} as const satisfies Record<string, SectionConfig>;

export type SettingsSection = keyof typeof SETTINGS_SECTIONS;

/** Top-level views; an open meeting covers whichever one is showing. */
export type AppView = "home" | "space" | "settings";

const NavItem: React.FC<{
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
  trailing?: React.ReactNode;
}> = ({ active, onClick, icon, label, trailing }) => (
  <button
    type="button"
    onClick={onClick}
    aria-current={active ? "page" : undefined}
    className={`group w-full flex items-center gap-2.5 h-8 px-2.5 rounded-lg text-[13.5px] text-start transition-colors cursor-pointer ${
      active
        ? "bg-surface-hover text-text"
        : "text-muted hover:text-text hover:bg-surface"
    }`}
  >
    <span
      className={`shrink-0 ${active ? "text-text" : "text-faint group-hover:text-muted"}`}
    >
      {icon}
    </span>
    <span className="flex-1 truncate">{label}</span>
    {trailing}
  </button>
);

interface SidebarProps {
  view: AppView;
  onNavigate: (view: AppView, projectId?: number) => void;
}

export const Sidebar: React.FC<SidebarProps> = ({ view, onNavigate }) => {
  const { t } = useTranslation();
  const projects = useProjectStore((s) => s.projects);
  const activeProjectId = useProjectStore((s) => s.activeProjectId);
  const createProject = useProjectStore((s) => s.createProject);
  const live = useMeetingStore((s) => s.live);
  const meetings = useMeetingStore((s) => s.meetings);
  const activeMeetingId = useMeetingStore((s) => s.activeMeetingId);
  const openMeetingProject = useMeetingStore(
    (s) => s.detail?.meeting.project_id ?? null,
  );
  const [adding, setAdding] = useState(false);
  const [draft, setDraft] = useState("");
  const [version, setVersion] = useState("");

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(""));
  }, []);

  const liveProjectId =
    live.meeting_id === null
      ? null
      : (meetings.find((m) => m.id === live.meeting_id)?.project_id ?? null);

  const submit = async () => {
    const name = draft.trim();
    setAdding(false);
    setDraft("");
    if (!name) return;
    const created = await createProject(name);
    if (created) onNavigate("space", created.id);
  };

  // An open meeting belongs to its space, or to Home when unfiled.
  const showing: AppView =
    activeMeetingId === null
      ? view
      : openMeetingProject === null
        ? "home"
        : "space";
  const currentSpace =
    activeMeetingId === null ? activeProjectId : openMeetingProject;

  return (
    <nav
      aria-label={t("nav.label")}
      className="flex flex-col w-60 shrink-0 h-full bg-sidebar border-e border-line"
    >
      <div
        data-tauri-drag-region
        className="h-12 shrink-0 flex items-center px-4 [[data-platform=macos]_&]:ps-[84px]"
      >
        <Wordmark className="pointer-events-none" />
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto px-2 pt-2 pb-4">
        <NavItem
          active={showing === "home"}
          onClick={() => onNavigate("home")}
          icon={<House className="w-4 h-4" />}
          label={t("nav.home")}
        />

        <div className="mt-6 mb-1 flex items-center justify-between ps-2.5 pe-1">
          <h2 className="text-xs font-medium text-faint">{t("nav.spaces")}</h2>
          <button
            type="button"
            title={t("nav.newSpace")}
            aria-label={t("nav.newSpace")}
            onClick={() => setAdding(true)}
            className="w-6 h-6 inline-flex items-center justify-center rounded-md text-faint hover:text-text hover:bg-surface transition-colors cursor-pointer"
          >
            <Plus className="w-3.5 h-3.5" />
          </button>
        </div>

        <div className="space-y-px">
          {projects.map((p) => (
            <NavItem
              key={p.id}
              active={showing === "space" && currentSpace === p.id}
              onClick={() => onNavigate("space", p.id)}
              icon={<Folder className="w-4 h-4" />}
              label={p.title}
              trailing={
                liveProjectId === p.id && live.status === "live" ? (
                  <span
                    className="w-1.5 h-1.5 rounded-full bg-accent live-dot"
                    title={t("meetings.status.live")}
                  />
                ) : undefined
              }
            />
          ))}
          {adding ? (
            <div className="flex items-center gap-2.5 h-8 px-2.5 rounded-lg bg-surface border border-line-strong">
              <Folder className="w-4 h-4 text-faint shrink-0" />
              <input
                autoFocus
                value={draft}
                placeholder={t("projects.newPlaceholder")}
                onChange={(e) => setDraft(e.target.value)}
                onBlur={submit}
                onKeyDown={(e) => {
                  if (e.key === "Enter") submit();
                  if (e.key === "Escape") {
                    setDraft("");
                    setAdding(false);
                  }
                }}
                className="flex-1 min-w-0 bg-transparent text-[13.5px] text-text placeholder:text-faint focus:outline-none"
              />
            </div>
          ) : (
            projects.length === 0 && (
              <p className="px-2.5 py-1.5 text-xs text-faint leading-relaxed">
                {t("nav.spacesEmpty")}
              </p>
            )
          )}
        </div>
      </div>

      <div className="shrink-0 px-2 pb-3 pt-2 border-t border-line space-y-1">
        <NavItem
          active={showing === "settings"}
          onClick={() => onNavigate("settings")}
          icon={<Settings className="w-4 h-4" />}
          label={t("nav.settings")}
        />
        <div className="px-2.5 pt-1 space-y-1.5 text-xs text-faint">
          <ModelSelector />
          <div className="flex items-center justify-between gap-2">
            <UpdateChecker />
            {version && (
              // eslint-disable-next-line i18next/no-literal-string
              <span className="font-mono text-[11px] tabular">v{version}</span>
            )}
          </div>
        </div>
      </div>
    </nav>
  );
};
