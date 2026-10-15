import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { Button } from "../../../ui/Button";

type PingState =
  | { status: "idle" }
  | { status: "testing" }
  | { status: "success"; reply: string; ms: number }
  | { status: "failure"; error: string };

/**
 * Sends "ping" to the saved provider and model and shows what came back, so
 * a pass means the assistant can actually reach the model.
 */
export const ModelPingButton: React.FC<{
  size?: "sm" | "md";
  onResult?: (ok: boolean) => void;
}> = ({ size = "md", onResult }) => {
  const { t } = useTranslation();
  const [ping, setPing] = useState<PingState>({ status: "idle" });

  const run = async () => {
    setPing({ status: "testing" });
    let next: PingState;
    try {
      const res = await commands.pingLlmModel();
      next =
        res.status === "ok"
          ? { status: "success", reply: res.data.reply, ms: res.data.ms }
          : { status: "failure", error: res.error };
    } catch (e) {
      next = {
        status: "failure",
        error: e instanceof Error ? e.message : String(e),
      };
    }
    setPing(next);
    onResult?.(next.status === "success");
  };

  return (
    <div className="flex items-center gap-3 min-w-0">
      <Button
        variant="secondary"
        size={size}
        onClick={run}
        disabled={ping.status === "testing"}
      >
        {ping.status === "testing"
          ? t("settings.assistant.provider.test.testing")
          : t("settings.assistant.provider.test.button")}
      </Button>
      <span role="status" aria-live="polite" className="text-sm min-w-0">
        {ping.status === "success" && (
          <span className="text-ok">
            {t("settings.assistant.provider.test.ok", {
              reply: ping.reply,
              seconds: (ping.ms / 1000).toFixed(1),
            })}
          </span>
        )}
        {ping.status === "failure" && (
          <span className="text-error break-words">
            {t("settings.assistant.provider.test.failure", {
              error: ping.error,
            })}
          </span>
        )}
      </span>
    </div>
  );
};
