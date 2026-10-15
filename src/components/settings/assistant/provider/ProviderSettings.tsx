import React from "react";
import { useTranslation } from "react-i18next";
import { RefreshCcw } from "lucide-react";
import { type LlmApiFormat } from "@/bindings";
import { Dropdown } from "../../../ui/Dropdown";
import { SettingContainer } from "../../../ui/SettingContainer";
import { SettingsGroup } from "../../../ui/SettingsGroup";
import { ResetButton } from "../../../ui/ResetButton";
import { ProviderSelect } from "./ProviderSelect";
import { BaseUrlField } from "./BaseUrlField";
import { ApiKeyField } from "./ApiKeyField";
import { ModelSelect } from "./ModelSelect";
import { ModelPingButton } from "./ModelPingButton";
import { useLlmProviderState } from "./useLlmProviderState";

/** The one LLM provider behind nudges, summaries, and image digests. */
export const ProviderSettings: React.FC = () => {
  const { t } = useTranslation();
  const state = useLlmProviderState();

  return (
    <SettingsGroup
      title={t("settings.assistant.provider.title")}
      description={t("settings.assistant.provider.description")}
    >
      <SettingContainer
        title={t("settings.assistant.provider.select.title")}
        description={t("settings.assistant.provider.select.description")}
        descriptionMode="tooltip"
        layout="horizontal"
        grouped={true}
      >
        <div className="flex items-center gap-2">
          <ProviderSelect
            options={state.providerOptions}
            value={state.selectedProviderId}
            onChange={state.handleProviderSelect}
          />
        </div>
      </SettingContainer>

      {state.isCustomProvider && (
        <SettingContainer
          title={t("settings.assistant.provider.apiFormat.title")}
          description={t("settings.assistant.provider.apiFormat.description")}
          descriptionMode="tooltip"
          layout="horizontal"
          grouped={true}
        >
          <Dropdown
            options={[
              {
                value: "openai",
                label: t(
                  "settings.assistant.provider.apiFormat.options.openai",
                ),
              },
              {
                value: "anthropic",
                label: t(
                  "settings.assistant.provider.apiFormat.options.anthropic",
                ),
              },
            ]}
            selectedValue={state.apiFormat}
            onSelect={(value) =>
              state.handleApiFormatChange(value as LlmApiFormat)
            }
            disabled={state.isApiFormatUpdating}
          />
        </SettingContainer>
      )}

      {state.isCustomProvider && (
        <SettingContainer
          title={t("settings.assistant.provider.baseUrl.title")}
          description={t("settings.assistant.provider.baseUrl.description")}
          descriptionMode="tooltip"
          layout="horizontal"
          grouped={true}
        >
          <div className="flex items-center gap-2">
            <BaseUrlField
              value={state.baseUrl}
              onBlur={state.handleBaseUrlChange}
              placeholder={
                state.apiFormat === "anthropic"
                  ? t(
                      "settings.assistant.provider.baseUrl.placeholderAnthropic",
                    )
                  : t("settings.assistant.provider.baseUrl.placeholder")
              }
              disabled={state.isBaseUrlUpdating}
              className="min-w-[380px]"
            />
          </div>
        </SettingContainer>
      )}

      <SettingContainer
        title={t("settings.assistant.provider.apiKey.title")}
        description={t("settings.assistant.provider.apiKey.description")}
        descriptionMode="tooltip"
        layout="horizontal"
        grouped={true}
      >
        <div className="flex items-center gap-2">
          <ApiKeyField
            value={state.apiKey}
            onBlur={state.handleApiKeyChange}
            placeholder={t(
              state.selectedProviderId === "openrouter"
                ? "settings.assistant.provider.apiKey.bundledPlaceholder"
                : "settings.assistant.provider.apiKey.placeholder",
            )}
            disabled={state.isApiKeyUpdating}
            className="min-w-[320px]"
          />
        </div>
      </SettingContainer>

      <SettingContainer
        title={t("settings.assistant.provider.model.title")}
        description={
          state.isCustomProvider
            ? t("settings.assistant.provider.model.descriptionCustom")
            : t("settings.assistant.provider.model.descriptionDefault")
        }
        descriptionMode="tooltip"
        layout="stacked"
        grouped={true}
      >
        <div className="flex items-center gap-2">
          <ModelSelect
            value={state.model}
            options={state.modelOptions}
            disabled={state.isModelUpdating}
            isLoading={state.isFetchingModels}
            placeholder={
              state.modelOptions.length > 0
                ? t("settings.assistant.provider.model.placeholderWithOptions")
                : t("settings.assistant.provider.model.placeholderNoOptions")
            }
            onSelect={state.handleModelSelect}
            onCreate={state.handleModelCreate}
            onBlur={() => {}}
            className="flex-1 min-w-[380px]"
          />
          <ResetButton
            onClick={state.handleRefreshModels}
            disabled={state.isFetchingModels}
            ariaLabel={t("settings.assistant.provider.model.refreshModels")}
            className="flex h-10 w-10 items-center justify-center"
          >
            <RefreshCcw
              className={`h-4 w-4 ${state.isFetchingModels ? "animate-spin" : ""}`}
            />
          </ResetButton>
        </div>
        <div className="mt-2">
          <ModelPingButton />
        </div>
      </SettingContainer>
    </SettingsGroup>
  );
};
