import React, { useEffect } from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import { LiveDock } from "@/components/meetings/LiveDock";
import { useMeetingStore } from "@/stores/meetingStore";
import {
  applyTheme,
  getStoredTheme,
  syncThemeFromSettings,
} from "@/lib/utils/theme";
import type { Theme } from "@/bindings";
import "@/i18n";
import "@/App.css";

// Its own webview, like the intervention card: theme and meeting state are
// loaded and followed here independently of the main window.
applyTheme(getStoredTheme());
syncThemeFromSettings();
listen<Theme>("theme-changed", (event) => applyTheme(event.payload));

/** The floating meeting bar shown while the app is in the background. */
const Pill: React.FC = () => {
  useEffect(() => {
    const store = useMeetingStore.getState();
    const unsubscribe = store.subscribe();
    store.refreshLive();
    store.loadMeetings(null);
    return () => {
      unsubscribe.then((f) => f());
    };
  }, []);
  return <LiveDock floating />;
};

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Pill />
  </React.StrictMode>,
);
