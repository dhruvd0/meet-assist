import { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type ObserverReadiness } from "@/bindings";
import { useMeetingStore } from "@/stores/meetingStore";
import { Button } from "../ui/Button";
import { SettingsGroup } from "../ui/SettingsGroup";
import { Textarea } from "../ui/Textarea";
import { parseTesterScript, seedTesterDraft } from "./LiveTesterPanel";

export function LiveTesterLauncher({
  ready,
}: {
  ready: ObserverReadiness | null;
}) {
  const { t } = useTranslation();
  const live = useMeetingStore((s) => s.live);
  const pending = useMeetingStore((s) => s.controlPending);
  const [script, setScript] = useState(() => t("meetings.tester.seed"));
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const parsed = parseTesterScript(script);
  const valid = parsed.kind === "valid" && parsed.turns.length > 0;
  const canStart =
    ready?.readiness === "ready" &&
    live.meeting_id === null &&
    pending === null &&
    valid &&
    !starting;

  const start = async () => {
    if (!canStart) return;
    setStarting(true);
    setError(null);
    try {
      const result = await commands.startTextTester();
      if (result.status !== "ok") {
        setError(result.error);
        return;
      }
      seedTesterDraft(result.data.id, script);
      const store = useMeetingStore.getState();
      await store.refreshLive();
      await store.openMeeting(result.data.id);
      await store.loadMeetings();
      const focused = await commands.focusMeeting(result.data.id);
      if (focused.status !== "ok") setError(focused.error);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setStarting(false);
    }
  };

  return (
    <SettingsGroup
      title={t("meetings.tester.title")}
      description={t("meetings.tester.description")}
    >
      <div className="p-4 space-y-3">
        <p className="text-sm text-muted">{t("meetings.tester.consent")}</p>
        <label htmlFor="tester-seed" className="block text-sm font-medium">
          {t("meetings.tester.conversation")}
        </label>
        <Textarea
          id="tester-seed"
          rows={7}
          className="w-full"
          value={script}
          disabled={starting}
          onChange={(event) => setScript(event.target.value)}
        />
        <p className="text-xs text-muted">{t("meetings.tester.scriptHint")}</p>
        {parsed.kind === "invalid" && (
          <p role="alert" className="text-sm text-error">
            {t("meetings.tester.invalid", { line: parsed.line })}
          </p>
        )}
        {ready && ready.readiness !== "ready" && (
          <p className="text-sm text-muted">
            {t(`meetings.observer.readiness.${ready.readiness}`, {
              detail: ready.detail ?? "",
            })}
          </p>
        )}
        {live.meeting_id !== null && (
          <p className="text-sm text-muted">
            {t("meetings.tester.alreadyRunning")}
          </p>
        )}
        {error && (
          <p role="alert" className="text-sm text-error">
            {error}
          </p>
        )}
        <Button disabled={!canStart} onClick={() => void start()}>
          {starting
            ? t("meetings.tester.starting")
            : t("meetings.tester.start")}
        </Button>
      </div>
    </SettingsGroup>
  );
}
