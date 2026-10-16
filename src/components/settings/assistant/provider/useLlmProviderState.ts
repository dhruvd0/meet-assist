import { useCallback, useEffect, useMemo, useState } from "react";
import { commands } from "@/bindings";
import { useSettings } from "../../../../hooks/useSettings";
import type { LlmApiFormat, LlmProvider } from "@/bindings";
import type { ModelOption } from "./types";
import type { DropdownOption } from "../../../ui/Dropdown";

type LlmProviderState = {
  /** The empty OpenRouter key field falls back to the build's bundled key. */
  usesBundledKey: boolean;
  providerOptions: DropdownOption[];
  selectedProviderId: string;
  selectedProvider: LlmProvider | undefined;
  isCustomProvider: boolean;
  baseUrl: string;
  handleBaseUrlChange: (value: string) => void;
  isBaseUrlUpdating: boolean;
  apiFormat: LlmApiFormat;
  handleApiFormatChange: (value: LlmApiFormat) => void;
  isApiFormatUpdating: boolean;
  apiKey: string;
  handleApiKeyChange: (value: string) => void;
  isApiKeyUpdating: boolean;
  model: string;
  handleModelChange: (value: string) => void;
  modelOptions: ModelOption[];
  isModelUpdating: boolean;
  isFetchingModels: boolean;
  handleProviderSelect: (providerId: string) => void;
  handleModelSelect: (value: string) => void;
  handleModelCreate: (value: string) => void;
  handleRefreshModels: () => void;
};

export const useLlmProviderState = (): LlmProviderState => {
  const {
    settings,
    isUpdating,
    setLlmProvider,
    updateLlmBaseUrl,
    updateLlmApiFormat,
    updateLlmApiKey,
    updateLlmModel,
    fetchLlmModels,
    llmModelOptions,
  } = useSettings();

  // Settings are guaranteed to have providers after migration
  const providers = settings?.llm_providers || [];

  const selectedProviderId = useMemo(() => {
    return settings?.llm_provider_id || providers[0]?.id || "openai";
  }, [providers, settings?.llm_provider_id]);

  const selectedProvider = useMemo(() => {
    return (
      providers.find((provider) => provider.id === selectedProviderId) ||
      providers[0]
    );
  }, [providers, selectedProviderId]);

  // Use settings directly as single source of truth
  const baseUrl = selectedProvider?.base_url ?? "";
  const apiFormat = selectedProvider?.api_format ?? "openai";
  const apiKey = settings?.llm_api_keys?.[selectedProviderId] ?? "";
  const model = settings?.llm_models?.[selectedProviderId] ?? "";

  const providerOptions = useMemo<DropdownOption[]>(() => {
    return providers.map((provider) => ({
      value: provider.id,
      label: provider.label,
    }));
  }, [providers]);

  const handleProviderSelect = useCallback(
    async (providerId: string) => {
      if (providerId === selectedProviderId) return;

      await setLlmProvider(providerId);

      // Auto-fetch available models for the new provider so the model dropdown
      // reflects what's actually valid. Without this, a stale model value from
      // a previous provider/base_url can persist and silently 404 at runtime.
      // Skip when the provider isn't configured yet (no API key / empty base URL)
      // to avoid unnecessary backend errors.
      const provider = providers.find((p) => p.id === providerId);
      const apiKey = settings?.llm_api_keys?.[providerId] ?? "";
      const hasBaseUrl = (provider?.base_url ?? "").trim() !== "";
      const hasApiKey = apiKey.trim() !== "";

      if (provider?.id === "custom" ? hasBaseUrl : hasApiKey) {
        void fetchLlmModels(providerId);
      }
    },
    [selectedProviderId, setLlmProvider, fetchLlmModels, providers, settings],
  );

  const handleBaseUrlChange = useCallback(
    (value: string) => {
      if (!selectedProvider || selectedProvider.id !== "custom") {
        return;
      }
      const trimmed = value.trim();
      if (trimmed && trimmed !== baseUrl) {
        void updateLlmBaseUrl(selectedProvider.id, trimmed);
      }
    },
    [selectedProvider, baseUrl, updateLlmBaseUrl],
  );

  const handleApiFormatChange = useCallback(
    (value: LlmApiFormat) => {
      if (value !== apiFormat) {
        void updateLlmApiFormat(selectedProviderId, value);
      }
    },
    [apiFormat, selectedProviderId, updateLlmApiFormat],
  );

  const handleApiKeyChange = useCallback(
    (value: string) => {
      const trimmed = value.trim();
      if (trimmed !== apiKey) {
        void updateLlmApiKey(selectedProviderId, trimmed);
      }
    },
    [apiKey, selectedProviderId, updateLlmApiKey],
  );

  const handleModelChange = useCallback(
    (value: string) => {
      const trimmed = value.trim();
      if (trimmed !== model) {
        void updateLlmModel(selectedProviderId, trimmed);
      }
    },
    [model, selectedProviderId, updateLlmModel],
  );

  const handleModelSelect = useCallback(
    (value: string) => {
      void updateLlmModel(selectedProviderId, value.trim());
    },
    [selectedProviderId, updateLlmModel],
  );

  const handleModelCreate = useCallback(
    (value: string) => {
      void updateLlmModel(selectedProviderId, value);
    },
    [selectedProviderId, updateLlmModel],
  );

  const handleRefreshModels = useCallback(() => {
    void fetchLlmModels(selectedProviderId);
  }, [fetchLlmModels, selectedProviderId]);

  const availableModelsRaw = llmModelOptions[selectedProviderId] || [];

  const modelOptions = useMemo<ModelOption[]>(() => {
    const seen = new Set<string>();
    const options: ModelOption[] = [];

    const upsert = (value: string | null | undefined) => {
      const trimmed = value?.trim();
      if (!trimmed || seen.has(trimmed)) return;
      seen.add(trimmed);
      options.push({ value: trimmed, label: trimmed });
    };

    // Add available models from API
    for (const candidate of availableModelsRaw) {
      upsert(candidate);
    }

    // Ensure current model is in the list
    upsert(model);

    return options;
  }, [availableModelsRaw, model]);

  const isBaseUrlUpdating = isUpdating(`llm_base_url:${selectedProviderId}`);
  const isApiFormatUpdating = isUpdating(
    `llm_api_format:${selectedProviderId}`,
  );
  const isApiKeyUpdating = isUpdating(`llm_api_key:${selectedProviderId}`);
  const isModelUpdating = isUpdating(`llm_model:${selectedProviderId}`);
  const isFetchingModels = isUpdating(`llm_models_fetch:${selectedProviderId}`);

  const isCustomProvider = selectedProvider?.id === "custom";

  // No automatic fetching - user must click refresh button

  // Builds made with a key fall back to it when the OpenRouter field is empty.
  const [bundledKey, setBundledKey] = useState(false);
  useEffect(() => {
    commands.hasBundledLlmKey().then(setBundledKey, () => setBundledKey(false));
  }, []);
  const usesBundledKey = bundledKey && selectedProviderId === "openrouter";

  return {
    usesBundledKey,
    providerOptions,
    selectedProviderId,
    selectedProvider,
    isCustomProvider,
    baseUrl,
    handleBaseUrlChange,
    isBaseUrlUpdating,
    apiFormat,
    handleApiFormatChange,
    isApiFormatUpdating,
    apiKey,
    handleApiKeyChange,
    isApiKeyUpdating,
    model,
    handleModelChange,
    modelOptions,
    isModelUpdating,
    isFetchingModels,
    handleProviderSelect,
    handleModelSelect,
    handleModelCreate,
    handleRefreshModels,
  };
};
