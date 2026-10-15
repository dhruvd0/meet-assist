import {
  useEffect,
  useLayoutEffect,
  useState,
  useRef,
  type ReactNode,
} from "react";
import { toast, Toaster } from "sonner";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkMicrophonePermission,
  requestMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { ModelStateEvent } from "./lib/types/events";
import "./App.css";
import { ArrowLeft } from "lucide-react";
import Onboarding, { AssistantOnboarding } from "./components/onboarding";
import { type OnboardingPreviewStep } from "./components/settings";
import { SettingsView } from "./components/settings/SettingsView";
import { ErrorBoundary } from "./components/ErrorBoundary";
import {
  Sidebar,
  type AppView,
  type SettingsSection,
} from "./components/Sidebar";
import { HomeView } from "./components/meetings/HomeView";
import { MeetingDetail } from "./components/meetings/MeetingDetail";
import { LiveDock } from "./components/meetings/LiveDock";
import { ProjectDetail } from "./components/projects/ProjectDetail";
import { WhatsNewGate } from "./components/whats-new";
import { useSettings } from "./hooks/useSettings";
import { useSettingsStore } from "./stores/settingsStore";
import { commands } from "@/bindings";
import { useMeetingStore } from "./stores/meetingStore";
import { useProjectStore } from "./stores/projectStore";
import { getLanguageDirection, initializeRTL } from "@/lib/utils/rtl";

type OnboardingStep = "model" | "assistant" | "done";

// How long to wait for an answer to the macOS microphone prompt before
// warning that access is denied.
const MIC_PERMISSION_WAIT_MS = 20_000;

// Stable identity so preview effects do not re-run due to callback changes.
const NOOP = () => {};

function App() {
  const { t, i18n } = useTranslation();
  const [onboardingStep, setOnboardingStep] = useState<OnboardingStep | null>(
    null,
  );
  const [onboardingPreview, setOnboardingPreview] =
    useState<OnboardingPreviewStep | null>(null);
  const [view, setView] = useState<AppView>("home");
  const [settingsSection, setSettingsSection] =
    useState<SettingsSection>("assistant");
  const activeMeetingId = useMeetingStore((s) => s.activeMeetingId);
  const closeMeeting = useMeetingStore((s) => s.closeMeeting);
  const liveMeetingId = useMeetingStore((s) => s.live.meeting_id);
  const liveStatus = useMeetingStore((s) => s.live.status);
  const { settings, updateSetting } = useSettings();
  const direction = getLanguageDirection(i18n.language);
  const refreshAudioDevices = useSettingsStore(
    (state) => state.refreshAudioDevices,
  );
  const refreshOutputDevices = useSettingsStore(
    (state) => state.refreshOutputDevices,
  );
  const hasCompletedPostOnboardingInit = useRef(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const isShowingOnboarding =
    onboardingPreview !== null ||
    onboardingStep === "model" ||
    onboardingStep === "assistant";

  // Classic scrollbars consume layout space. Reserve a matching gutter on the
  // opposite edge while onboarding is visible so its content stays centered in
  // the physical window. Overlay scrollbars ignore scrollbar-gutter.
  useLayoutEffect(() => {
    const attribute = "data-onboarding-active";
    document.documentElement.toggleAttribute(attribute, isShowingOnboarding);
    return () => document.documentElement.removeAttribute(attribute);
  }, [isShowingOnboarding]);

  // Each page opens at its top.
  const activeProjectId = useProjectStore((s) => s.activeProjectId);
  useLayoutEffect(() => {
    scrollRef.current?.scrollTo({ top: 0 });
  }, [view, activeProjectId, activeMeetingId, settingsSection]);

  const navigate = (next: AppView, projectId?: number) => {
    closeMeeting();
    if (next === "space" && projectId !== undefined)
      useProjectStore.getState().openProject(projectId);
    setView(next);
  };

  useEffect(() => {
    checkOnboardingStatus();
  }, []);

  // Initialize RTL direction when language changes
  useEffect(() => {
    initializeRTL(i18n.language);
  }, [i18n.language]);

  // Initialize shortcuts and refresh audio devices when main app loads
  useEffect(() => {
    if (onboardingStep === "done" && !hasCompletedPostOnboardingInit.current) {
      hasCompletedPostOnboardingInit.current = true;
      commands.initializeShortcuts().catch((e) => {
        console.warn("Failed to initialize:", e);
      });
      refreshAudioDevices();
      refreshOutputDevices();
    }
  }, [onboardingStep, refreshAudioDevices, refreshOutputDevices]);

  // Meetings and spaces are shown on every page (sidebar, feed, dock), so the
  // shell subscribes to their events once for the whole session.
  const ready = onboardingStep === "done";
  useEffect(() => {
    if (!ready) return;
    const meetings = useMeetingStore.getState();
    const projects = useProjectStore.getState();
    meetings.loadMeetings(null);
    meetings.refreshLive();
    projects.loadProjects();
    const unMeetings = meetings.subscribe();
    const unProjects = projects.subscribe();
    return () => {
      unMeetings.then((fn) => fn());
      unProjects.then((fn) => fn());
    };
  }, [ready]);

  // A meeting starting or ending changes the feed.
  useEffect(() => {
    if (ready) useMeetingStore.getState().loadMeetings(null);
  }, [ready, liveMeetingId, liveStatus]);

  // Handle keyboard shortcuts for debug mode toggle
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // Check for Ctrl+Shift+D (Windows/Linux) or Cmd+Shift+D (macOS)
      const isDebugShortcut =
        event.shiftKey &&
        event.key.toLowerCase() === "d" &&
        (event.ctrlKey || event.metaKey);

      if (isDebugShortcut) {
        event.preventDefault();
        const currentDebugMode = settings?.debug_mode ?? false;
        updateSetting("debug_mode", !currentDebugMode);
      }
    };

    // Add event listener when component mounts
    document.addEventListener("keydown", handleKeyDown);

    // Cleanup event listener when component unmounts
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [settings?.debug_mode, updateSetting]);

  // A shortcut, tray, or CLI start that failed opens the window; say why.
  useEffect(() => {
    const unlisten = listen<string>("meeting-toggle-error", (e) => {
      toast.error(t("errors.meetingToggleFailedTitle"), {
        description: e.payload,
      });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [t]);

  // Listen for model loading failures and show a toast
  useEffect(() => {
    const unlisten = listen<ModelStateEvent>("model-state-changed", (event) => {
      if (event.payload.event_type === "loading_failed") {
        toast.error(
          t("errors.modelLoadFailed", {
            model:
              event.payload.model_name || t("errors.modelLoadFailedUnknown"),
          }),
          {
            description: event.payload.error,
          },
        );
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [t]);

  // Meetings need the microphone. Ask early so the first meeting doesn't
  // stall on a system prompt; a denial surfaces as a toast, not a gate.
  const ensureMicrophonePermission = async () => {
    const currentPlatform = platform();
    try {
      if (currentPlatform === "macos") {
        if (!(await checkMicrophonePermission())) {
          // Returns before the user answers (and at once if access was
          // already denied), so poll for the outcome before warning.
          await requestMicrophonePermission();
          const deadline = Date.now() + MIC_PERMISSION_WAIT_MS;
          let granted = false;
          while (!granted && Date.now() < deadline) {
            await new Promise((r) => setTimeout(r, 500));
            granted = await checkMicrophonePermission();
          }
          if (!granted) {
            toast.error(t("errors.micPermissionDeniedTitle"), {
              description: t("errors.micPermissionDenied.macos"),
            });
          }
        }
      } else if (currentPlatform === "windows") {
        const status = await commands.getWindowsMicrophonePermissionStatus();
        if (status.supported && status.overall_access === "denied") {
          toast.error(t("errors.micPermissionDeniedTitle"), {
            description: t("errors.micPermissionDenied.windows"),
          });
        }
      }
    } catch (e) {
      console.warn("Failed to check microphone permission:", e);
    }
  };

  const checkOnboardingStatus = async () => {
    try {
      const settingsResult = await commands.getAppSettings();
      const hasCompletedOnboarding =
        settingsResult.status === "ok" &&
        settingsResult.data.onboarding_completed === true;
      setOnboardingStep(hasCompletedOnboarding ? "done" : "model");
    } catch (error) {
      console.error("Failed to check onboarding status:", error);
      setOnboardingStep("model");
    }
    // After a step is set, so the Toaster is mounted for any warning.
    void ensureMicrophonePermission();
  };

  const handleModelSelected = () => {
    // User has started a download; new users now pick an assistant provider
    setOnboardingStep("assistant");
  };

  const handleAssistantComplete = () => {
    setOnboardingStep("done");
  };

  // Rendered once around every step below (including onboarding) so
  // toast.error() calls surface to the user. sonner renders via a portal, so
  // its position in the tree doesn't affect layout. Without this, errors during
  // onboarding (e.g. a model download failing because blob.handy.computer is
  // unreachable) are silently swallowed and the wizard just appears to "blink".
  const toaster = (
    <Toaster
      theme="system"
      toastOptions={{
        unstyled: true,
        classNames: {
          toast:
            "bg-surface text-text border border-line-strong rounded-xl shadow-[0_16px_40px_-12px_rgb(0_0_0/0.45)] px-4 py-3 flex items-center gap-3 text-sm w-[356px]",
          title: "font-medium",
          description: "text-muted",
          actionButton:
            "h-7 px-2.5 text-xs font-medium rounded-lg bg-text text-background hover:bg-text/85 cursor-pointer whitespace-nowrap",
        },
      }}
    />
  );

  // Still checking onboarding status
  if (onboardingStep === null) {
    return null;
  }

  // Select the content for the current step. The Toaster is rendered once, in a
  // stable wrapper around this node, so crossing between onboarding steps and
  // the main app never remounts it (which would drop any in-flight toast).
  let content: ReactNode;
  if (onboardingPreview) {
    // Render previews in the same top-level slot as real onboarding. Keeping
    // the settings layout unmounted ensures viewport overflow behaves exactly
    // as it does during first-run onboarding.
    content = (
      <>
        <Onboarding onModelSelected={NOOP} preview />
        <button
          type="button"
          onClick={() => setOnboardingPreview(null)}
          className="fixed top-4 end-4 z-50 rounded-lg border border-line-strong bg-surface px-4 py-2 text-sm font-medium text-text shadow-lg hover:bg-surface-hover cursor-pointer"
        >
          {t("settings.debug.onboardingPreview.exitButton")}
        </button>
      </>
    );
  } else if (onboardingStep === "model") {
    content = <Onboarding onModelSelected={handleModelSelected} />;
  } else if (onboardingStep === "assistant") {
    content = <AssistantOnboarding onComplete={handleAssistantComplete} />;
  } else {
    let page: ReactNode;
    if (activeMeetingId !== null) page = <MeetingDetail />;
    else if (view === "space" && activeProjectId !== null)
      page = <ProjectDetail onDeleted={() => navigate("home")} />;
    else if (view === "settings")
      page = (
        <SettingsView
          section={settingsSection}
          onSectionChange={setSettingsSection}
          onPreviewOnboarding={setOnboardingPreview}
        />
      );
    else page = <HomeView />;

    content = (
      <div
        dir={direction}
        className="h-screen flex bg-background select-none cursor-default"
      >
        <ErrorBoundary context="What's New">
          <WhatsNewGate />
        </ErrorBoundary>
        <Sidebar view={view} onNavigate={navigate} />
        <main className="relative flex-1 min-w-0 flex flex-col">
          <div
            data-tauri-drag-region
            className="h-12 shrink-0 flex items-center px-4"
          >
            {activeMeetingId !== null && (
              <button
                type="button"
                onClick={closeMeeting}
                className="inline-flex items-center gap-1.5 h-8 ps-2 pe-3 rounded-full border border-line text-[13px] text-muted hover:text-text hover:border-line-strong transition-colors cursor-pointer"
              >
                <ArrowLeft className="w-3.5 h-3.5 rtl:rotate-180" />
                {t("nav.back")}
              </button>
            )}
          </div>
          <div ref={scrollRef} className="@container flex-1 overflow-y-auto">
            {page}
          </div>
          <LiveDock />
        </main>
      </div>
    );
  }

  return (
    <>
      {toaster}
      {content}
    </>
  );
}

export default App;
