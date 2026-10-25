import React from "react";
import { useTranslation } from "react-i18next";
import type { CloudState } from "@/bindings";
import { useCloudStore } from "@/stores/cloudStore";

const DOT: Record<Exclude<CloudState, "signed_out">, string> = {
  idle: "bg-ok",
  syncing: "bg-accent animate-pulse",
  offline: "bg-faint",
  error: "bg-error",
  account_mismatch: "bg-warning",
};

/** Top-bar account badge: initial plus a sync dot, or a quiet sign-in link. */
export const AccountChip: React.FC<{ onOpen: () => void }> = ({ onOpen }) => {
  const { t } = useTranslation();
  const status = useCloudStore((s) => s.status);
  if (!status) return null;

  if (status.state === "signed_out") {
    return (
      <button
        type="button"
        onClick={onOpen}
        className="h-7 px-2.5 rounded-full text-[12.5px] text-faint hover:text-text transition-colors cursor-pointer"
      >
        {t("account.signIn")}
      </button>
    );
  }

  const label = `${status.email ?? ""} · ${t(`account.state.${status.state}`)}`;
  return (
    <button
      type="button"
      onClick={onOpen}
      title={label}
      aria-label={label}
      className="relative h-7 w-7 rounded-full border border-line bg-surface text-[12px] font-medium text-muted uppercase hover:border-line-strong hover:text-text transition-colors cursor-pointer"
    >
      {(status.email ?? "?").charAt(0)}
      <span
        className={`absolute -bottom-0.5 -end-0.5 h-2.5 w-2.5 rounded-full ring-2 ring-background ${DOT[status.state]}`}
      />
    </button>
  );
};
