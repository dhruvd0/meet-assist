import { create } from "zustand";
import { subscribeWithSelector } from "zustand/middleware";
import { listen } from "@tauri-apps/api/event";
import type {
  AppSettings as Settings,
  AudioDevice,
  LlmApiFormat,
  ChineseScript,
  TranscribeAcceleratorSetting,
  ModelUnloadTimeout,
  OrtAcceleratorSetting,
  VadBackend,
} from "@/bindings";
import { commands } from "@/bindings";
import { toast } from "sonner";
import { ok, toastError } from "@/lib/result";

interface SettingsStore {
  settings: Settings | null;
  defaultSettings: Settings | null;
  isLoading: boolean;
  isUpdating: Record<string, boolean>;
  audioDevices: AudioDevice[];
  outputDevices: AudioDevice[];
  customSounds: { start: boolean; stop: boolean };
  llmModelOptions: Record<string, string[]>;
  // null until loadUpdateChecksLocked() resolves
  updateChecksLocked: boolean | null;

  // Actions
  initialize: () => Promise<void>;
  loadDefaultSettings: () => Promise<void>;
  loadUpdateChecksLocked: () => Promise<void>;
  updateSetting: <K extends keyof Settings>(
    key: K,
    value: Settings[K],
  ) => Promise<void>;
  resetSetting: (key: keyof Settings) => Promise<void>;
  refreshSettings: () => Promise<void>;
  refreshAudioDevices: () => Promise<void>;
  refreshOutputDevices: () => Promise<void>;
  updateBinding: (id: string, binding: string) => Promise<void>;
  resetBinding: (id: string) => Promise<void>;
  getSetting: <K extends keyof Settings>(key: K) => Settings[K] | undefined;
  isUpdatingKey: (key: string) => boolean;
  playTestSound: (soundType: "start" | "stop") => Promise<void>;
  checkCustomSounds: () => Promise<void>;
  setLlmProvider: (providerId: string) => Promise<void>;
  updateLlmSetting: (
    settingType: "base_url" | "api_key" | "model",
    providerId: string,
    value: string,
  ) => Promise<void>;
  updateLlmBaseUrl: (providerId: string, baseUrl: string) => Promise<void>;
  updateLlmApiFormat: (
    providerId: string,
    apiFormat: LlmApiFormat,
  ) => Promise<void>;
  updateLlmApiKey: (providerId: string, apiKey: string) => Promise<void>;
  updateLlmModel: (providerId: string, model: string) => Promise<void>;
  fetchLlmModels: (providerId: string) => Promise<string[]>;
  setLlmModelOptions: (providerId: string, models: string[]) => void;

  // Internal state setters
  setSettings: (settings: Settings | null) => void;
  setDefaultSettings: (defaultSettings: Settings | null) => void;
  setLoading: (loading: boolean) => void;
  setUpdating: (key: string, updating: boolean) => void;
  setAudioDevices: (devices: AudioDevice[]) => void;
  setOutputDevices: (devices: AudioDevice[]) => void;
  setCustomSounds: (sounds: { start: boolean; stop: boolean }) => void;
}

// Note: Default settings are now fetched from Rust via commands.getDefaultSettings()
// This ensures platform-specific defaults (like shortcuts) work correctly

const DEFAULT_AUDIO_DEVICE: AudioDevice = {
  index: "default",
  name: "Default",
  is_default: true,
};

const settingUpdaters: {
  [K in keyof Settings]?: (value: Settings[K]) => Promise<unknown>;
} = {
  audio_feedback: (value) =>
    commands.changeAudioFeedbackSetting(value as boolean),
  audio_feedback_volume: (value) =>
    commands.changeAudioFeedbackVolumeSetting(value as number),
  sound_theme: (value) => commands.changeSoundThemeSetting(value as string),
  start_hidden: (value) => commands.changeStartHiddenSetting(value as boolean),
  autostart_enabled: (value) =>
    commands.changeAutostartSetting(value as boolean),
  update_checks_enabled: (value) =>
    commands.changeUpdateChecksSetting(value as boolean),
  show_whats_new_on_update: (value) =>
    commands.changeShowWhatsNewOnUpdateSetting(value as boolean),
  whats_new_last_seen_version: (value) =>
    commands.changeWhatsNewLastSeenVersionSetting(value as string),
  selected_microphone: (value) =>
    commands.setSelectedMicrophone(
      (value as string) === "Default" || value === null
        ? "default"
        : (value as string),
    ),
  selected_channel: async (value) => {
    const result = await commands.setSelectedChannel(
      (value as number | null | undefined) ?? null,
    );
    if (result.status === "error") {
      throw new Error(result.error);
    }
  },
  selected_output_device: (value) =>
    commands.setSelectedOutputDevice(
      (value as string) === "Default" || value === null
        ? "default"
        : (value as string),
    ),
  translate_to_english: (value) =>
    commands.changeTranslateToEnglishSetting(value as boolean),
  selected_language: (value) =>
    commands.changeSelectedLanguageSetting(value as string),
  debug_mode: (value) => commands.changeDebugModeSetting(value as boolean),
  custom_words: (value) => commands.updateCustomWords(value as string[]),
  word_correction_threshold: (value) =>
    commands.changeWordCorrectionThresholdSetting(value as number),
  meeting_assistant_enabled: (value) =>
    commands.changeMeetingAssistantSetting(value as boolean),
  meeting_system_audio_enabled: (value) =>
    commands.changeMeetingSystemAudioSetting(value as boolean),
  meeting_vision_enabled: (value) =>
    commands.changeMeetingVisionSetting(value as boolean),
  meeting_sensitivity: (value) =>
    commands.changeMeetingSensitivitySetting(
      value as "low" | "medium" | "high",
    ),
  model_unload_timeout: (value) =>
    commands.setModelUnloadTimeout(value as ModelUnloadTimeout),
  meeting_observer_interval_secs: (value) =>
    commands.changeMeetingObserverIntervalSetting(value as number),
  meeting_max_line_secs: (value) =>
    commands.changeMeetingMaxLineSetting(value as number),
  meeting_beep_enabled: (value) =>
    commands.changeMeetingBeepSetting(value as boolean),
  log_level: (value) => commands.setLogLevel(value as any),
  app_language: (value) => commands.changeAppLanguageSetting(value as string),
  theme: (value) => commands.changeThemeSetting(value as string),
  experimental_enabled: (value) =>
    commands.changeExperimentalEnabledSetting(value as boolean),
  vad_enabled: (value) => commands.changeVadEnabledSetting(value as boolean),
  vad_backend: async (value) => {
    const result = await commands.changeVadBackendSetting(value as VadBackend);
    if (result.status === "error") {
      // Rejected switches (e.g. mid-recording) roll the dropdown back via the
      // throw below; the toast tells the user why.
      toast.error(result.error);
      throw new Error(result.error);
    }
  },
  filler_word_removal_enabled: (value) =>
    commands.changeFillerWordRemovalEnabledSetting(value as boolean),
  chinese_script: (value) =>
    commands.changeChineseScriptSetting(value as ChineseScript),
  show_tray_icon: (value) =>
    commands.changeShowTrayIconSetting(value as boolean),
  transcribe_accelerator: (value) =>
    commands.changeTranscribeAcceleratorSetting(
      value as TranscribeAcceleratorSetting,
    ),
  ort_accelerator: (value) =>
    commands.changeOrtAcceleratorSetting(value as OrtAcceleratorSetting),
  transcribe_gpu_device: (value) =>
    commands.changeTranscribeGpuDevice(value as string | null),
};

export const useSettingsStore = create<SettingsStore>()(
  subscribeWithSelector((set, get) => ({
    settings: null,
    defaultSettings: null,
    isLoading: true,
    isUpdating: {},
    audioDevices: [],
    outputDevices: [],
    customSounds: { start: false, stop: false },
    llmModelOptions: {},
    updateChecksLocked: null,

    // Internal setters
    setSettings: (settings) => set({ settings }),
    setDefaultSettings: (defaultSettings) => set({ defaultSettings }),
    setLoading: (isLoading) => set({ isLoading }),
    setUpdating: (key, updating) =>
      set((state) => ({
        isUpdating: { ...state.isUpdating, [key]: updating },
      })),
    setAudioDevices: (audioDevices) => set({ audioDevices }),
    setOutputDevices: (outputDevices) => set({ outputDevices }),
    setCustomSounds: (customSounds) => set({ customSounds }),

    // Getters
    getSetting: (key) => get().settings?.[key],
    isUpdatingKey: (key) => get().isUpdating[key] || false,

    // Load settings from store
    refreshSettings: async () => {
      try {
        const result = await commands.getAppSettings();
        if (result.status === "ok") {
          const settings = result.data;
          const normalizedSettings: Settings = {
            ...settings,
            selected_microphone: settings.selected_microphone ?? "Default",
            selected_output_device:
              settings.selected_output_device ?? "Default",
          };
          set({ settings: normalizedSettings, isLoading: false });
        } else {
          toastError("errors.settingsLoadFailed", result.error);
          set({ isLoading: false });
        }
      } catch (error) {
        toastError("errors.settingsLoadFailed", error);
        set({ isLoading: false });
      }
    },

    // Load audio devices
    refreshAudioDevices: async () => {
      try {
        const result = await commands.getAvailableMicrophones();
        if (result.status === "ok") {
          const devicesWithDefault = [
            DEFAULT_AUDIO_DEVICE,
            ...result.data.filter(
              (d) => d.name !== "Default" && d.name !== "default",
            ),
          ];
          set({ audioDevices: devicesWithDefault });
        } else {
          toastError("errors.devicesLoadFailed", result.error);
          set({ audioDevices: [DEFAULT_AUDIO_DEVICE] });
        }
      } catch (error) {
        toastError("errors.devicesLoadFailed", error);
        set({ audioDevices: [DEFAULT_AUDIO_DEVICE] });
      }
    },

    // Load output devices
    refreshOutputDevices: async () => {
      try {
        const result = await commands.getAvailableOutputDevices();
        if (result.status === "ok") {
          const devicesWithDefault = [
            DEFAULT_AUDIO_DEVICE,
            ...result.data.filter(
              (d) => d.name !== "Default" && d.name !== "default",
            ),
          ];
          set({ outputDevices: devicesWithDefault });
        } else {
          toastError("errors.devicesLoadFailed", result.error);
          set({ outputDevices: [DEFAULT_AUDIO_DEVICE] });
        }
      } catch (error) {
        toastError("errors.devicesLoadFailed", error);
        set({ outputDevices: [DEFAULT_AUDIO_DEVICE] });
      }
    },

    // Play a test sound
    playTestSound: async (soundType: "start" | "stop") => {
      try {
        await ok(commands.playTestSound(soundType));
      } catch (error) {
        toastError("errors.soundTestFailed", error);
      }
    },

    checkCustomSounds: async () => {
      try {
        const sounds = await commands.checkCustomSounds();
        get().setCustomSounds(sounds);
      } catch (error) {
        console.error("Failed to check custom sounds:", error);
      }
    },

    // Update a specific setting
    updateSetting: async <K extends keyof Settings>(
      key: K,
      value: Settings[K],
    ) => {
      const { settings, setUpdating } = get();
      const updateKey = String(key);
      const originalValue = settings?.[key];

      setUpdating(updateKey, true);

      try {
        set((state) => ({
          settings: state.settings ? { ...state.settings, [key]: value } : null,
        }));

        const updater = settingUpdaters[key];
        if (updater) {
          await ok(updater(value));
        } else if (key !== "bindings" && key !== "selected_model") {
          console.warn(`No handler for setting: ${String(key)}`);
        }
      } catch (error) {
        toastError("errors.settingSaveFailed", error);
        set((state) => ({
          settings: state.settings
            ? { ...state.settings, [key]: originalValue }
            : null,
        }));
      } finally {
        setUpdating(updateKey, false);
      }
    },

    // Reset a setting to its default value
    resetSetting: async (key) => {
      const { defaultSettings } = get();
      if (defaultSettings) {
        const defaultValue = defaultSettings[key];
        if (defaultValue !== undefined) {
          await get().updateSetting(key, defaultValue as any);
        }
      }
    },

    // Update a specific binding
    updateBinding: async (id, binding) => {
      const { settings, setUpdating } = get();
      const updateKey = `binding_${id}`;
      const originalBinding = settings?.bindings?.[id]?.current_binding;

      setUpdating(updateKey, true);

      try {
        // Optimistic update
        set((state) => ({
          settings: state.settings
            ? {
                ...state.settings,
                bindings: {
                  ...state.settings.bindings,
                  [id]: {
                    ...state.settings.bindings?.[id]!,
                    current_binding: binding,
                  },
                },
              }
            : null,
        }));

        const result = await commands.changeBinding(id, binding);

        // Check if the command executed successfully
        if (result.status === "error") {
          throw new Error(result.error);
        }

        // Check if the binding change was successful
        if (!result.data.success) {
          throw new Error(result.data.error || "Failed to update binding");
        }
      } catch (error) {
        console.error(`Failed to update binding ${id}:`, error);

        // Rollback on error
        if (originalBinding && get().settings) {
          set((state) => ({
            settings: state.settings
              ? {
                  ...state.settings,
                  bindings: {
                    ...state.settings.bindings,
                    [id]: {
                      ...state.settings.bindings?.[id]!,
                      current_binding: originalBinding,
                    },
                  },
                }
              : null,
          }));
        }

        // Re-throw to let the caller know it failed
        throw error;
      } finally {
        setUpdating(updateKey, false);
      }
    },

    // Reset a specific binding
    resetBinding: async (id) => {
      const { setUpdating, refreshSettings } = get();
      const updateKey = `binding_${id}`;

      setUpdating(updateKey, true);

      try {
        await ok(commands.resetBinding(id));
        await refreshSettings();
      } catch (error) {
        toastError("errors.settingSaveFailed", error);
      } finally {
        setUpdating(updateKey, false);
      }
    },

    setLlmProvider: async (providerId) => {
      const { settings, setUpdating, refreshSettings, setLlmModelOptions } =
        get();
      const updateKey = "llm_provider_id";
      const previousId = settings?.llm_provider_id ?? null;

      setUpdating(updateKey, true);

      if (settings) {
        set((state) => ({
          settings: state.settings
            ? { ...state.settings, llm_provider_id: providerId }
            : null,
        }));
      }

      // Clear cached model options for the new provider so the dropdown
      // doesn't show stale models from a previous fetch or base_url.
      setLlmModelOptions(providerId, []);

      try {
        await ok(commands.setLlmProvider(providerId));
        await refreshSettings();
      } catch (error) {
        toastError("errors.settingSaveFailed", error);
        if (previousId !== null) {
          set((state) => ({
            settings: state.settings
              ? { ...state.settings, llm_provider_id: previousId }
              : null,
          }));
        }
      } finally {
        setUpdating(updateKey, false);
      }
    },

    // Generic updater for post-processing provider settings
    updateLlmSetting: async (
      settingType: "base_url" | "api_key" | "model",
      providerId: string,
      value: string,
    ) => {
      const { setUpdating, refreshSettings } = get();
      const updateKey = `llm_${settingType}:${providerId}`;

      setUpdating(updateKey, true);

      try {
        if (settingType === "base_url") {
          await ok(commands.changeLlmBaseUrlSetting(providerId, value));
        } else if (settingType === "api_key") {
          await ok(commands.changeLlmApiKeySetting(providerId, value));
        } else if (settingType === "model") {
          await ok(commands.changeLlmModelSetting(providerId, value));
        }
        await refreshSettings();
      } catch (error) {
        toastError("errors.settingSaveFailed", error);
        await refreshSettings();
      } finally {
        setUpdating(updateKey, false);
      }
    },

    updateLlmBaseUrl: async (providerId, baseUrl) => {
      const { setUpdating, refreshSettings } = get();
      const updateKey = `llm_base_url:${providerId}`;

      setUpdating(updateKey, true);

      try {
        // Persist the new base URL first.
        await ok(commands.changeLlmBaseUrlSetting(providerId, baseUrl));

        // Reset the stored model since the previous value is almost certainly
        // invalid for the new endpoint (e.g. switching Custom from Groq to
        // Cerebras). Only proceed if the reset succeeds.
        await ok(commands.changeLlmModelSetting(providerId, ""));

        // Clear cached model options only after both backend writes succeed.
        set((state) => ({
          llmModelOptions: {
            ...state.llmModelOptions,
            [providerId]: [],
          },
        }));

        // Single refresh after both backend writes.
        await refreshSettings();
      } catch (error) {
        toastError("errors.settingSaveFailed", error);
        await refreshSettings();
      } finally {
        setUpdating(updateKey, false);
      }
    },

    updateLlmApiFormat: async (providerId, apiFormat) => {
      const { setUpdating, refreshSettings } = get();
      const updateKey = `llm_api_format:${providerId}`;

      // The format decides the auth headers, so models listed under the old
      // format may not reflect this one - user should click refresh after
      set((state) => ({
        llmModelOptions: {
          ...state.llmModelOptions,
          [providerId]: [],
        },
      }));
      setUpdating(updateKey, true);

      try {
        await ok(commands.changeLlmApiFormatSetting(providerId, apiFormat));
        await refreshSettings();
      } catch (error) {
        toastError("errors.settingSaveFailed", error);
        await refreshSettings();
      } finally {
        setUpdating(updateKey, false);
      }
    },

    updateLlmApiKey: async (providerId, apiKey) => {
      // Clear cached models when API key changes - user should click refresh after
      set((state) => ({
        llmModelOptions: {
          ...state.llmModelOptions,
          [providerId]: [],
        },
      }));
      return get().updateLlmSetting("api_key", providerId, apiKey);
    },

    updateLlmModel: async (providerId, model) => {
      return get().updateLlmSetting("model", providerId, model);
    },

    fetchLlmModels: async (providerId) => {
      const updateKey = `llm_models_fetch:${providerId}`;
      const { setUpdating, setLlmModelOptions } = get();

      setUpdating(updateKey, true);

      try {
        // Call Tauri backend command instead of fetch
        const result = await commands.fetchLlmModels(providerId);
        if (result.status === "ok") {
          setLlmModelOptions(providerId, result.data);
          return result.data;
        }
        toastError("errors.modelListFailed", result.error);
        return [];
      } catch (error) {
        // Don't cache empty array on error - let user retry
        toastError("errors.modelListFailed", error);
        return [];
      } finally {
        setUpdating(updateKey, false);
      }
    },

    setLlmModelOptions: (providerId, models) =>
      set((state) => ({
        llmModelOptions: {
          ...state.llmModelOptions,
          [providerId]: models,
        },
      })),

    // Load default settings from Rust
    loadDefaultSettings: async () => {
      try {
        const result = await commands.getDefaultSettings();
        if (result.status === "ok") {
          set({ defaultSettings: result.data });
        } else {
          console.error("Failed to load default settings:", result.error);
        }
      } catch (error) {
        console.error("Failed to load default settings:", error);
      }
    },

    // Check whether update checks are locked by system configuration
    // (e.g. HANDY_DISABLE_UPDATER, set by the Nix package)
    loadUpdateChecksLocked: async () => {
      try {
        const locked = await commands.isUpdateChecksLocked();
        set({ updateChecksLocked: locked });
      } catch (error) {
        console.error("Failed to check update checks lock state:", error);
        // Fail open: an unknown lock state means "not locked", otherwise the
        // update checker waits for it forever and checks never start.
        set({ updateChecksLocked: false });
      }
    },

    // Initialize everything
    initialize: async () => {
      const {
        refreshSettings,
        checkCustomSounds,
        loadDefaultSettings,
        loadUpdateChecksLocked,
      } = get();

      // Note: Audio devices are NOT refreshed here. The frontend (App.tsx)
      // is responsible for calling refreshAudioDevices/refreshOutputDevices
      // after onboarding completes. This avoids triggering permission dialogs
      // on macOS before the user is ready.
      await Promise.all([
        loadDefaultSettings(),
        refreshSettings(),
        checkCustomSounds(),
        loadUpdateChecksLocked(),
      ]);

      // Re-fetch settings when the backend changes them (e.g. language
      // reset during model switch). The backend is the source of truth.
      listen("model-state-changed", () => {
        get().refreshSettings();
      });
      listen<{ setting?: string }>("settings-changed", (event) => {
        get().refreshSettings();
        if (event.payload.setting === "selected_microphone") {
          get().refreshAudioDevices();
        }
      });
    },
  })),
);
