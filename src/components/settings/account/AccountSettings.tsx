import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type CloudState } from "@/bindings";
import { useCloudStore } from "@/stores/cloudStore";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";

const STATE_TONE: Record<CloudState, string> = {
  signed_out: "text-faint",
  idle: "text-muted",
  syncing: "text-muted",
  offline: "text-faint",
  error: "text-error",
  account_mismatch: "text-error",
};

/** Optional account: signing in turns on background sync of meetings and settings. */
export const AccountSettings: React.FC = () => {
  const { t, i18n } = useTranslation();
  const status = useCloudStore((s) => s.status);
  const apply = useCloudStore((s) => s.apply);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState<"in" | "up" | "out" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const submit = async (mode: "in" | "up") => {
    setBusy(mode);
    setError(null);
    const res =
      mode === "in"
        ? await commands.cloudSignIn(email, password)
        : await commands.cloudSignUp(email, password);
    setBusy(null);
    if (res.status === "ok") {
      setPassword("");
      apply(res.data);
    } else {
      setError(res.error);
    }
  };

  const signOut = async () => {
    setBusy("out");
    const res = await commands.cloudSignOut();
    setBusy(null);
    if (res.status === "ok") apply(res.data);
  };

  const signedIn = status && status.state !== "signed_out";

  if (!signedIn) {
    const canSubmit = email.includes("@") && password.length >= 8 && !busy;
    return (
      <div className="max-w-3xl w-full mx-auto space-y-6">
        <SettingsGroup title={t("account.title")}>
          <form
            className="p-4 space-y-3"
            onSubmit={(e) => {
              e.preventDefault();
              if (canSubmit) void submit("in");
            }}
          >
            <p className="text-[13px] text-muted">{t("account.intro")}</p>
            {status?.message && (
              <p className="text-[13px] text-faint">{status.message}</p>
            )}
            <Input
              type="email"
              autoComplete="email"
              placeholder={t("account.email")}
              aria-label={t("account.email")}
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              className="w-full"
            />
            <Input
              type="password"
              autoComplete="current-password"
              placeholder={t("account.password")}
              aria-label={t("account.password")}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              className="w-full"
            />
            <p className="text-[12px] text-faint">
              {t("account.passwordHint")}
            </p>
            {error && (
              <p role="alert" className="text-[13px] text-error">
                {error}
              </p>
            )}
            <div className="flex gap-2">
              <Button type="submit" disabled={!canSubmit}>
                {busy === "in" ? t("account.signingIn") : t("account.signIn")}
              </Button>
              <Button
                type="button"
                variant="secondary"
                disabled={!canSubmit}
                onClick={() => void submit("up")}
              >
                {busy === "up" ? t("account.creating") : t("account.create")}
              </Button>
            </div>
          </form>
        </SettingsGroup>
      </div>
    );
  }

  const lastSynced = status.last_synced_at
    ? new Date(status.last_synced_at).toLocaleString(i18n.language)
    : null;

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("account.title")}>
        <div className="p-4 space-y-3">
          <p className="text-[13px] text-muted">
            {t("account.signedInAs", { email: status.email ?? "" })}
          </p>
          <p className={`text-[13px] ${STATE_TONE[status.state]}`}>
            {t(`account.state.${status.state}`)}
            {status.message && status.state !== "idle"
              ? `: ${status.message}`
              : ""}
          </p>
          <p className="text-[12px] text-faint">
            {lastSynced
              ? t("account.lastSynced", { time: lastSynced })
              : t("account.neverSynced")}
            {status.pending > 0 &&
              ` · ${t("account.pending", { count: status.pending })}`}
          </p>
          <div className="flex gap-2">
            <Button
              variant="secondary"
              disabled={status.state === "syncing" || !!busy}
              onClick={() => void commands.cloudSyncNow()}
            >
              {t("account.syncNow")}
            </Button>
            <Button
              variant="ghost"
              disabled={!!busy}
              onClick={() => void signOut()}
            >
              {t("account.signOut")}
            </Button>
          </div>
        </div>
      </SettingsGroup>
    </div>
  );
};
