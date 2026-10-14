import React from "react";
import { useTranslation } from "react-i18next";
import { MicrophoneSelector } from "../MicrophoneSelector";
import { ChannelSelector } from "../ChannelSelector";
import { ShortcutInput } from "../ShortcutInput";
import { OutputDeviceSelector } from "../OutputDeviceSelector";
import { AudioFeedback } from "../AudioFeedback";
import { VolumeSlider } from "../VolumeSlider";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { useSettings } from "../../../hooks/useSettings";

/** Capture: the meeting shortcut, what we listen to, and what we play back. */
export const AudioSettings: React.FC = () => {
  const { t } = useTranslation();
  const { audioFeedbackEnabled, getSetting, updateSetting, isUpdating } =
    useSettings();

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.general.shortcut.title")}>
        <ShortcutInput shortcutId="toggle_meeting" grouped={true} />
      </SettingsGroup>
      <SettingsGroup title={t("settings.sound.title")}>
        <MicrophoneSelector descriptionMode="tooltip" grouped={true} />
        <ChannelSelector descriptionMode="tooltip" grouped={true} />
        <ToggleSwitch
          checked={getSetting("meeting_system_audio_enabled") ?? true}
          onChange={(v) => updateSetting("meeting_system_audio_enabled", v)}
          isUpdating={isUpdating("meeting_system_audio_enabled")}
          label={t("settings.meeting.systemAudio.label")}
          description={t("settings.meeting.systemAudio.description")}
          descriptionMode="tooltip"
          grouped={true}
        />
        <AudioFeedback descriptionMode="tooltip" grouped={true} />
        <OutputDeviceSelector
          descriptionMode="tooltip"
          grouped={true}
          disabled={!audioFeedbackEnabled}
        />
        <VolumeSlider disabled={!audioFeedbackEnabled} />
      </SettingsGroup>
    </div>
  );
};
