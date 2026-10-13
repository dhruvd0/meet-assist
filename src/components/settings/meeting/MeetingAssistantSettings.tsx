import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { MeetingSensitivity } from "@/bindings";
import { useSettings } from "../../../hooks/useSettings";
import { Dropdown } from "../../ui/Dropdown";
import { Input } from "../../ui/Input";
import { SettingContainer } from "../../ui/SettingContainer";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { ToggleSwitch } from "../../ui/ToggleSwitch";

const MIN_INTERVAL = 10;
const MAX_INTERVAL = 120;

export const MeetingAssistantSettings: React.FC = () => {
  const { t } = useTranslation();
  const { settings, getSetting, updateSetting, isUpdating } = useSettings();

  const interval = getSetting("meeting_observer_interval_secs") ?? 30;
  const [draft, setDraft] = useState(String(interval));
  useEffect(() => setDraft(String(interval)), [interval]);

  const providerOptions = [
    { value: "", label: t("settings.meeting.provider.default") },
    ...(settings?.post_process_providers ?? []).map((p) => ({
      value: p.id,
      label: p.label,
    })),
  ];

  const sensitivityOptions = (["low", "medium", "high"] as const).map((v) => ({
    value: v,
    label: t(`settings.meeting.sensitivity.${v}`),
  }));

  const commitInterval = () => {
    const n = Math.round(Number(draft));
    if (!Number.isFinite(n) || draft.trim() === "") {
      setDraft(String(interval));
      return;
    }
    const clamped = Math.min(MAX_INTERVAL, Math.max(MIN_INTERVAL, n));
    setDraft(String(clamped));
    if (clamped !== interval)
      updateSetting("meeting_observer_interval_secs", clamped);
  };

  return (
    <SettingsGroup
      title={t("settings.meeting.title")}
      description={t("settings.meeting.description")}
    >
      <SettingContainer
        title={t("settings.meeting.provider.label")}
        description={t("settings.meeting.provider.description")}
        descriptionMode="tooltip"
        grouped={true}
      >
        <Dropdown
          options={providerOptions}
          selectedValue={getSetting("meeting_provider_id") ?? ""}
          onSelect={(v) => updateSetting("meeting_provider_id", v)}
          disabled={isUpdating("meeting_provider_id")}
        />
      </SettingContainer>
      <SettingContainer
        title={t("settings.meeting.sensitivity.label")}
        description={t("settings.meeting.sensitivity.description")}
        descriptionMode="tooltip"
        grouped={true}
      >
        <Dropdown
          options={sensitivityOptions}
          selectedValue={getSetting("meeting_sensitivity") ?? "medium"}
          onSelect={(v) =>
            updateSetting("meeting_sensitivity", v as MeetingSensitivity)
          }
          disabled={isUpdating("meeting_sensitivity")}
        />
      </SettingContainer>
      <SettingContainer
        title={t("settings.meeting.interval.label")}
        description={t("settings.meeting.interval.description")}
        descriptionMode="tooltip"
        grouped={true}
      >
        <Input
          type="number"
          variant="compact"
          className="w-24"
          min={MIN_INTERVAL}
          max={MAX_INTERVAL}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commitInterval}
          onKeyDown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur();
          }}
          disabled={isUpdating("meeting_observer_interval_secs")}
        />
      </SettingContainer>
      <ToggleSwitch
        checked={getSetting("meeting_beep_enabled") ?? true}
        onChange={(v) => updateSetting("meeting_beep_enabled", v)}
        isUpdating={isUpdating("meeting_beep_enabled")}
        label={t("settings.meeting.beep.label")}
        description={t("settings.meeting.beep.description")}
        descriptionMode="tooltip"
        grouped={true}
      />
    </SettingsGroup>
  );
};
