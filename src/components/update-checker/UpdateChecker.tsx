import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { arch, platform } from "@tauri-apps/plugin-os";
import { toast } from "sonner";
import i18n from "@/i18n";
import { toastError } from "@/lib/result";
import { useSettings } from "@/hooks/useSettings";
import { useMeetingStore } from "@/stores/meetingStore";
import { commands } from "@/bindings";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import {
  resolvePortableInstallerUrl,
  PORTABLE_RELEASES_URL,
} from "./portableInstaller";

const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

type UpdateState =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "available"; update: Update }
  | { phase: "portable"; url: string }
  | { phase: "downloading"; update: Update; progress: number | null }
  | { phase: "ready"; update: Update }
  | { phase: "installing"; update: Update }
  | { phase: "restart" };

const UpdateChecker: React.FC<{ className?: string }> = ({
  className = "",
}) => {
  const { t } = useTranslation();
  const { settings, isLoading, updateChecksLocked } = useSettings();
  const enabled =
    !isLoading &&
    settings?.update_checks_enabled === true &&
    updateChecksLocked === false;
  const meetingRunning = useMeetingStore((s) => s.live.meeting_id !== null);
  const [state, setState] = useState<UpdateState>({ phase: "idle" });
  const [showPortableDialog, setShowPortableDialog] = useState(false);
  const current = useRef<UpdateState>(state);
  const generation = useRef(0);
  const transition = useCallback((next: UpdateState) => {
    current.current = next;
    setState(next);
  }, []);

  const checkForUpdates = useCallback(
    async (manual = false) => {
      if (!enabled || current.current.phase !== "idle") return;
      const run = generation.current;
      const active = () => generation.current === run;
      let update: Update | null = null;
      transition({ phase: "checking" });
      try {
        update = await check({ timeout: 30_000 });
        if (!active()) return;
        if (!update) {
          transition({ phase: "idle" });
          if (manual) toast.success(i18n.t("footer.upToDate"));
          return;
        }
        const portable = await commands.isPortable();
        if (!active()) return;
        if (portable) {
          transition({
            phase: "portable",
            url: resolvePortableInstallerUrl(
              update.rawJson,
              platform(),
              arch(),
            ),
          });
          await update.close();
          update = null;
          if (manual) setShowPortableDialog(true);
          return;
        }
        let downloaded = 0;
        let total = 0;
        const packageUpdate = update;
        transition({ phase: "downloading", update, progress: null });
        await update.download(
          (event) => {
            if (!active()) return;
            if (event.event === "Started") {
              downloaded = 0;
              total = event.data.contentLength ?? 0;
            } else if (event.event === "Progress") {
              downloaded += event.data.chunkLength;
            }
            transition({
              phase: "downloading",
              update: packageUpdate,
              progress:
                total > 0
                  ? Math.min(100, Math.round((downloaded / total) * 100))
                  : null,
            });
          },
          { timeout: 300_000 },
        );
        if (active()) transition({ phase: "ready", update });
      } catch (error) {
        if (active()) {
          transition(
            update ? { phase: "available", update } : { phase: "idle" },
          );
          toastError(
            update ? "errors.updateDownloadFailed" : "errors.updateCheckFailed",
            error,
          );
        }
      } finally {
        // A disabled updater or unmounted sidebar must not retain a late download.
        if (!active() && update) await update.close().catch(() => {});
      }
    },
    [enabled, transition],
  );

  useEffect(() => {
    if (!enabled) return;
    void checkForUpdates();
    const timer = window.setInterval(
      () => void checkForUpdates(),
      CHECK_INTERVAL_MS,
    );
    const unlisten = listen("check-for-updates", () => {
      if (current.current.phase === "portable") setShowPortableDialog(true);
      else void checkForUpdates(true);
    });
    return () => {
      generation.current += 1;
      window.clearInterval(timer);
      void unlisten.then((stop) => stop());
      const previous = current.current;
      transition({ phase: "idle" });
      // In-flight operations release their resource when they finish.
      if (previous.phase === "ready" || previous.phase === "available") {
        void previous.update.close().catch(() => {});
      }
    };
  }, [enabled, checkForUpdates, transition]);

  const installUpdate = async () => {
    const pending = current.current;
    if (!enabled || (pending.phase !== "ready" && pending.phase !== "restart"))
      return;
    // Read the backend immediately before installing; the sidebar's event state
    // can lag a meeting started from the global shortcut or tray.
    try {
      if ((await commands.meetingState()).meeting_id !== null) {
        toast.info(t("footer.updateAfterMeeting"));
        return;
      }
      if (!enabled || current.current !== pending) return;
      if (pending.phase === "ready") {
        transition({ phase: "installing", update: pending.update });
        await pending.update.install();
        await pending.update.close();
        transition({ phase: "restart" });
      }
      await relaunch();
    } catch (error) {
      if (current.current.phase === "installing") {
        await current.current.update.close().catch(() => {});
        transition({ phase: "idle" });
      }
      toastError("errors.updateInstallFailed", error);
    }
  };

  let label: string;
  let action: (() => void) | undefined;
  if (!enabled) label = t("footer.updateCheckingDisabled");
  else {
    switch (state.phase) {
      case "idle":
        label = t("footer.checkForUpdates");
        action = () => void checkForUpdates(true);
        break;
      case "checking":
        label = t("footer.checkingUpdates");
        break;
      case "downloading":
        label =
          state.progress === null
            ? t("footer.downloadingUnknown")
            : t("footer.downloading", { progress: state.progress });
        break;
      case "available":
        label = t("footer.retryDownload");
        action = () => {
          void state.update.close().catch(() => {});
          transition({ phase: "idle" });
          void checkForUpdates(true);
        };
        break;
      case "portable":
        label = t("footer.updateAvailableShort");
        action = () => setShowPortableDialog(true);
        break;
      case "ready":
      case "restart":
        label = t(
          meetingRunning
            ? "footer.updateAfterMeeting"
            : "footer.restartToUpdate",
        );
        if (!meetingRunning) action = () => void installUpdate();
        break;
      case "installing":
        label = t("footer.installing");
        break;
    }
  }
  const portableUrl =
    state.phase === "portable" ? state.url : PORTABLE_RELEASES_URL;
  const directInstaller = portableUrl !== PORTABLE_RELEASES_URL;

  return (
    <>
      <Dialog
        open={showPortableDialog && enabled && state.phase === "portable"}
        onOpenChange={setShowPortableDialog}
        title={t("footer.portableUpdateTitle")}
        closeLabel={t("common.close")}
        footer={
          <>
            <Button
              variant="secondary"
              onClick={() => setShowPortableDialog(false)}
            >
              {t("common.close")}
            </Button>
            <Button
              onClick={() => {
                void openUrl(portableUrl)
                  .then(() => setShowPortableDialog(false))
                  .catch((error: unknown) =>
                    toastError("errors.updateDownloadFailed", error),
                  );
              }}
            >
              {t(
                directInstaller
                  ? "footer.portableUpdateButton"
                  : "footer.portableUpdateBrowseButton",
              )}
            </Button>
          </>
        }
      >
        <p className="text-sm text-muted">
          {t(
            directInstaller
              ? "footer.portableUpdateMessage"
              : "footer.portableUpdateBrowseMessage",
          )}
        </p>
      </Dialog>
      <div className={`min-w-0 text-xs ${className}`}>
        {action ? (
          <button
            onClick={action}
            className={`text-start transition-colors ${state.phase === "idle" ? "text-muted hover:text-text" : "text-accent hover:text-accent/80 font-medium"}`}
          >
            {label}
          </button>
        ) : (
          <span role="status" className="text-muted tabular-nums">
            {label}
          </span>
        )}
      </div>
    </>
  );
};

export default UpdateChecker;
