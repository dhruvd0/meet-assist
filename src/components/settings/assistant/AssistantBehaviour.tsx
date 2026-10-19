import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands, type MeetingSensitivity } from "@/bindings";
import { useSettingsStore } from "@/stores/settingsStore";
import { ApiKeyField } from "./provider/ApiKeyField";
import { useSettings } from "../../../hooks/useSettings";
import { Dropdown } from "../../ui/Dropdown";
import { Input } from "../../ui/Input";
import { SettingContainer } from "../../ui/SettingContainer";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { ToggleSwitch } from "../../ui/ToggleSwitch";

const MIN_INTERVAL = 10;
const MAX_INTERVAL = 120;
const DEFAULT_INTERVAL_SECS = 20;
const MIN_LINE = 5;
const MAX_LINE = 60;
const DEFAULT_LINE_SECS = 30;

/** A whole number of seconds, clamped to [min, max] and saved on blur. */
const SecondsInput: React.FC<{
  value: number;
  min: number;
  max: number;
  disabled: boolean;
  onCommit: (secs: number) => void;
}> = ({ value, min, max, disabled, onCommit }) => {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);

  const commit = () => {
    const n = Math.round(Number(draft));
    if (!Number.isFinite(n) || draft.trim() === "") {
      setDraft(String(value));
      return;
    }
    const clamped = Math.min(max, Math.max(min, n));
    setDraft(String(clamped));
    if (clamped !== value) onCommit(clamped);
  };

  return (
    <Input
      type="number"
      variant="compact"
      className="w-24"
      min={min}
      max={max}
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
      }}
      disabled={disabled}
    />
  );
};

/** When and how loudly the assistant speaks up during a meeting. */
export const AssistantBehaviour: React.FC = () => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const defaultSettings = useSettingsStore((store) => store.defaultSettings);
  const updateJevApiKey = useSettingsStore((store) => store.updateJevApiKey);
  const [bundledJev, setBundledJev] = useState(false);
  useEffect(() => {
    void commands
      .hasBundledJevKey()
      .then(setBundledJev)
      .catch(() => setBundledJev(false));
  }, []);
  const providerId = getSetting("llm_provider_id");
  const provider = getSetting("llm_providers")?.find(
    (item) => item.id === providerId,
  );
  const builtin = defaultSettings?.llm_providers?.find(
    (item) => item.id === providerId && item.id !== "custom",
  );
  const cloudProvider = (() => {
    if (!provider || !builtin) return false;
    try {
      const url = new URL(provider.base_url);
      const expected = new URL(builtin.base_url);
      return (
        url.protocol === "https:" &&
        url.origin === expected.origin &&
        url.pathname.replace(/\/$/, "") ===
          expected.pathname.replace(/\/$/, "") &&
        !url.username &&
        !url.password &&
        !url.search &&
        !url.hash
      );
    } catch {
      return false;
    }
  })();
  const jevKey = getSetting("llm_api_keys")?.jev ?? "";

  const interval =
    getSetting("meeting_observer_interval_secs") ?? DEFAULT_INTERVAL_SECS;
  const enabled = getSetting("meeting_assistant_enabled") ?? false;

  const sensitivityOptions = (["low", "medium", "high"] as const).map((v) => ({
    value: v,
    label: t(`settings.meeting.sensitivity.${v}`),
  }));

  return (
    <SettingsGroup
      title={t("settings.meeting.title")}
      description={t("settings.meeting.description")}
    >
      <ToggleSwitch
        checked={enabled}
        onChange={(v) => updateSetting("meeting_assistant_enabled", v)}
        isUpdating={isUpdating("meeting_assistant_enabled")}
        label={t("settings.meeting.enabled.label")}
        description={t("meetings.jev.consent", { interval })}
        descriptionMode="inline"
        grouped={true}
      />
      <ToggleSwitch
        checked={getSetting("meeting_jev_enabled") ?? true}
        onChange={(value) => updateSetting("meeting_jev_enabled", value)}
        isUpdating={isUpdating("meeting_jev_enabled")}
        disabled={!enabled || !cloudProvider}
        label={t("meetings.jev.label")}
        description={t(
          cloudProvider ? "meetings.jev.description" : "meetings.jev.cloudOnly",
        )}
        descriptionMode="inline"
        grouped={true}
      />
      <SettingContainer
        title={t("meetings.jev.keyLabel")}
        description={t("meetings.jev.keyDescription")}
        descriptionMode="inline"
        grouped={true}
      >
        <ApiKeyField
          value={jevKey}
          onBlur={(value) => {
            if (value !== jevKey) void updateJevApiKey(value);
          }}
          disabled={!enabled || !cloudProvider || isUpdating("jev_api_key")}
          placeholder={t(
            bundledJev ? "meetings.jev.bundledKey" : "meetings.jev.ownKey",
          )}
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
          disabled={!enabled || isUpdating("meeting_sensitivity")}
        />
      </SettingContainer>
      <SettingContainer
        title={t("settings.meeting.interval.label")}
        description={t("settings.meeting.interval.description")}
        descriptionMode="tooltip"
        grouped={true}
      >
        <SecondsInput
          value={interval}
          min={MIN_INTERVAL}
          max={MAX_INTERVAL}
          disabled={!enabled || isUpdating("meeting_observer_interval_secs")}
          onCommit={(v) => updateSetting("meeting_observer_interval_secs", v)}
        />
      </SettingContainer>
      <SettingContainer
        title={t("settings.meeting.maxLine.label")}
        description={t("settings.meeting.maxLine.description")}
        descriptionMode="tooltip"
        grouped={true}
      >
        <SecondsInput
          value={getSetting("meeting_max_line_secs") ?? DEFAULT_LINE_SECS}
          min={MIN_LINE}
          max={MAX_LINE}
          disabled={isUpdating("meeting_max_line_secs")}
          onCommit={(v) => updateSetting("meeting_max_line_secs", v)}
        />
      </SettingContainer>
      <ToggleSwitch
        checked={getSetting("meeting_beep_enabled") ?? true}
        onChange={(v) => updateSetting("meeting_beep_enabled", v)}
        isUpdating={isUpdating("meeting_beep_enabled")}
        disabled={!enabled}
        label={t("settings.meeting.beep.label")}
        description={t("settings.meeting.beep.description")}
        descriptionMode="tooltip"
        grouped={true}
      />
    </SettingsGroup>
  );
};
