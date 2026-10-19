import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type ObserverReadiness } from "@/bindings";
import { useSettings } from "../../../hooks/useSettings";
import { ProviderSettings } from "./provider";
import { AssistantBehaviour } from "./AssistantBehaviour";
import { LiveTesterLauncher } from "../../meetings/LiveTesterLauncher";

/** Provider first, behaviour second: one page, one pass. */
export const AssistantSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const [ready, setReady] = useState<ObserverReadiness | null>(null);

  // Re-check whenever any setting changes (provider, key, model, opt-in).
  useEffect(() => {
    commands
      .getObserverReadiness()
      .then(setReady)
      .catch((e) => console.warn("Failed to check assistant readiness:", e));
  }, [settings]);

  const blocked =
    ready !== null &&
    ready.readiness !== "ready" &&
    ready.readiness !== "disabled";

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      {blocked && (
        <p role="alert" className="px-4 py-2 text-sm text-error">
          {t(`meetings.observer.readiness.${ready.readiness}`, {
            detail: ready.detail ?? "",
          })}
        </p>
      )}
      <ProviderSettings />
      <AssistantBehaviour />
      <LiveTesterLauncher ready={ready} />
    </div>
  );
};
