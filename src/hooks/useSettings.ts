import { useEffect } from "react";
import { useSettingsStore } from "../stores/settingsStore";
import type {
  AppSettings as Settings,
  AudioDevice,
  LlmApiFormat,
} from "@/bindings";

interface UseSettingsReturn {
  // State
  settings: Settings | null;
  isLoading: boolean;
  isUpdating: (key: string) => boolean;
  audioDevices: AudioDevice[];
  outputDevices: AudioDevice[];
  audioFeedbackEnabled: boolean;
  llmModelOptions: Record<string, string[]>;
  updateChecksLocked: boolean | null;

  // Actions
  updateSetting: <K extends keyof Settings>(
    key: K,
    value: Settings[K],
  ) => Promise<void>;
  resetSetting: (key: keyof Settings) => Promise<void>;
  refreshSettings: () => Promise<void>;
  refreshAudioDevices: () => Promise<void>;
  refreshOutputDevices: () => Promise<void>;

  // Binding-specific actions
  updateBinding: (id: string, binding: string) => Promise<void>;
  resetBinding: (id: string) => Promise<void>;

  // Convenience getters
  getSetting: <K extends keyof Settings>(key: K) => Settings[K] | undefined;

  // Post-processing helpers
  setLlmProvider: (providerId: string) => Promise<void>;
  updateLlmBaseUrl: (providerId: string, baseUrl: string) => Promise<void>;
  updateLlmApiFormat: (
    providerId: string,
    apiFormat: LlmApiFormat,
  ) => Promise<void>;
  updateLlmApiKey: (providerId: string, apiKey: string) => Promise<void>;
  updateLlmModel: (providerId: string, model: string) => Promise<void>;
  fetchLlmModels: (providerId: string) => Promise<string[]>;
}

export const useSettings = (): UseSettingsReturn => {
  const store = useSettingsStore();

  // Initialize on first mount
  useEffect(() => {
    if (store.isLoading) {
      store.initialize();
    }
  }, [store.initialize, store.isLoading]);

  return {
    settings: store.settings,
    isLoading: store.isLoading,
    isUpdating: store.isUpdatingKey,
    audioDevices: store.audioDevices,
    outputDevices: store.outputDevices,
    audioFeedbackEnabled: store.settings?.audio_feedback || false,
    llmModelOptions: store.llmModelOptions,
    updateChecksLocked: store.updateChecksLocked,
    updateSetting: store.updateSetting,
    resetSetting: store.resetSetting,
    refreshSettings: store.refreshSettings,
    refreshAudioDevices: store.refreshAudioDevices,
    refreshOutputDevices: store.refreshOutputDevices,
    updateBinding: store.updateBinding,
    resetBinding: store.resetBinding,
    getSetting: store.getSetting,
    setLlmProvider: store.setLlmProvider,
    updateLlmBaseUrl: store.updateLlmBaseUrl,
    updateLlmApiFormat: store.updateLlmApiFormat,
    updateLlmApiKey: store.updateLlmApiKey,
    updateLlmModel: store.updateLlmModel,
    fetchLlmModels: store.fetchLlmModels,
  };
};
