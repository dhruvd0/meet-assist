import React from "react";
import { useTranslation } from "react-i18next";
import { useSettings } from "@/hooks/useSettings";
import { SETTINGS_SECTIONS, type SettingsSection } from "../Sidebar";
import { DebugSettings } from "./debug/DebugSettings";
import type { OnboardingPreviewStep } from "./debug/OnboardingPreview";

/** Settings as one page with a tab per section. */
export const SettingsView: React.FC<{
  section: SettingsSection;
  onSectionChange: (section: SettingsSection) => void;
  onPreviewOnboarding: (step: OnboardingPreviewStep) => void;
}> = ({ section, onSectionChange, onPreviewOnboarding }) => {
  const { t } = useTranslation();
  const { settings } = useSettings();

  const tabs = (
    Object.entries(SETTINGS_SECTIONS) as [
      SettingsSection,
      (typeof SETTINGS_SECTIONS)[SettingsSection],
    ][]
  ).filter(([, c]) => c.enabled(settings));
  const current = tabs.some(([id]) => id === section) ? section : "assistant";
  const Active = SETTINGS_SECTIONS[current].component;

  return (
    <div className="w-full max-w-[720px] mx-auto px-8 pt-6 pb-24 page-in">
      <h1 className="font-serif text-[32px] leading-tight tracking-[-0.01em] text-text">
        {t("nav.settings")}
      </h1>
      <div
        role="tablist"
        aria-label={t("nav.settings")}
        className="mt-5 mb-8 flex flex-wrap gap-1 border-b border-line"
      >
        {tabs.map(([id, c]) => (
          <button
            key={id}
            role="tab"
            type="button"
            aria-selected={current === id}
            onClick={() => onSectionChange(id)}
            className={`relative h-9 px-3 text-[13.5px] transition-colors cursor-pointer ${
              current === id ? "text-text" : "text-faint hover:text-muted"
            }`}
          >
            {t(c.labelKey)}
            {current === id && (
              <span className="absolute inset-x-2 -bottom-px h-0.5 rounded-full bg-text" />
            )}
          </button>
        ))}
      </div>
      <div role="tabpanel">
        {current === "debug" ? (
          <DebugSettings onPreviewOnboarding={onPreviewOnboarding} />
        ) : (
          <Active />
        )}
      </div>
    </div>
  );
};
