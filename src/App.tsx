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
import Footer from "./components/footer";
import Onboarding, { AssistantOnboarding } from "./components/onboarding";
import {
  DebugSettings,
  type OnboardingPreviewStep,
} from "./components/settings";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { Sidebar, SidebarSection, SECTIONS_CONFIG } from "./components/Sidebar";
import { WhatsNewGate } from "./components/whats-new";
import { useSettings } from "./hooks/useSettings";
import { useSettingsStore } from "./stores/settingsStore";
import { commands, events } from "@/bindings";
import { useMeetingStore } from "./stores/meetingStore";
import { getLanguageDirection, initializeRTL } from "@/lib/utils/rtl";

type OnboardingStep = "model" | "assistant" | "done";

// How long to wait for an answer to the macOS microphone prompt before
// warning that access is denied.
const MIC_PERMISSION_WAIT_MS = 20_000;

// Stable identity so preview effects do not re-run due to callback changes.
const NOOP = () => {};

const renderSettingsContent = (
  section: SidebarSection,
  onPreviewOnboarding: (step: OnboardingPreviewStep) => void,
) => {
  if (section === "debug") {
    return <DebugSettings onPreviewOnboarding={onPreviewOnboarding} />;
  }

  const ActiveComponent =
    SECTIONS_CONFIG[section]?.component || SECTIONS_CONFIG.general.component;
  return <ActiveComponent />;
};

function App() {
  const { t, i18n } = useTranslation();
  const [onboardingStep, setOnboardingStep] = useState<OnboardingStep | null>(
    null,
  );
  const [onboardingPreview, setOnboardingPreview] =
    useState<OnboardingPreviewStep | null>(null);
  const [currentSection, setCurrentSection] =
    useState<SidebarSection>("projects");
  const { settings, updateSetting } = useSettings();
  const direction = getLanguageDirection(i18n.language);
  const refreshAudioDevices = useSettingsStore(
    (state) => state.refreshAudioDevices,
  );
  const refreshOutputDevices = useSettingsStore(
    (state) => state.refreshOutputDevices,
  );
  const hasCompletedPostOnboardingInit = useRef(false);
  const settingsScrollRef = useRef<HTMLDivElement>(null);
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

  // Reset the scroll position whenever the active section changes.
  useLayoutEffect(() => {
    settingsScrollRef.current?.scrollTo({ top: 0 });
  }, [currentSection]);

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

  // "Open meeting" on the intervention card: show the Meetings section with
  // that meeting open (the section may not be mounted to hear it itself).
  useEffect(() => {
    const unlisten = events.openMeetingPayload.listen((e) => {
      useMeetingStore.getState().openMeeting(e.payload.meeting_id);
      setCurrentSection("meetings");
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Rust asks the window to jump to a section (e.g. toggle with no projects).
  useEffect(() => {
    const unlisten = listen<string>("navigate-section", (e) => {
      if (e.payload in SECTIONS_CONFIG) {
        setCurrentSection(e.payload as SidebarSection);
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

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
            "bg-background border border-mid-gray/20 rounded-lg shadow-lg px-4 py-3 flex items-center gap-3 text-sm",
          title: "font-medium",
          description: "text-mid-gray",
          actionButton:
            "px-2 py-1 text-xs font-medium rounded-lg border bg-mid-gray/10 border-mid-gray/20 hover:bg-background-ui/30 hover:border-logo-primary cursor-pointer whitespace-nowrap",
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
          className="fixed top-4 end-4 z-50 rounded-lg border border-mid-gray/20 bg-background px-4 py-2 text-sm font-medium text-text shadow-lg hover:bg-background-ui/30 cursor-pointer"
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
    content = (
      <div
        dir={direction}
        className="h-screen flex flex-col select-none cursor-default"
      >
        <ErrorBoundary context="What's New">
          <WhatsNewGate />
        </ErrorBoundary>
        {/* Main content area that takes remaining space */}
        <div className="flex-1 flex overflow-hidden">
          <Sidebar
            activeSection={currentSection}
            onSectionChange={setCurrentSection}
          />
          {/* Scrollable content area */}
          <div className="flex-1 flex flex-col overflow-hidden">
            <div ref={settingsScrollRef} className="flex-1 overflow-y-auto">
              <div className="flex flex-col items-center p-4 gap-4">
                {renderSettingsContent(currentSection, setOnboardingPreview)}
              </div>
            </div>
          </div>
        </div>
        {/* Fixed footer at bottom */}
        <Footer />
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
