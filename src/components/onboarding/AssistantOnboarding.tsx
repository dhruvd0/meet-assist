import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui/Button";
import { ProviderSelect } from "../settings/assistant/provider/ProviderSelect";
import { BaseUrlField } from "../settings/assistant/provider/BaseUrlField";
import { ApiKeyField } from "../settings/assistant/provider/ApiKeyField";
import { ModelSelect } from "../settings/assistant/provider/ModelSelect";
import { useLlmProviderState } from "../settings/assistant/provider/useLlmProviderState";
import { ModelPingButton } from "../settings/assistant/provider/ModelPingButton";

interface AssistantOnboardingProps {
  onComplete: () => void;
}

export const AssistantOnboarding: React.FC<AssistantOnboardingProps> = ({
  onComplete,
}) => {
  const { t } = useTranslation();
  const state = useLlmProviderState();
  const [pinged, setPinged] = useState(false);

  const canContinue = Boolean(state.model) || pinged;

  return (
    <div className="h-screen w-screen flex flex-col items-center justify-center p-6 gap-6">
      <div className="max-w-xl w-full flex flex-col gap-2 text-center">
        <h2 className="text-xl font-semibold">
          {t("onboarding.assistant.title")}
        </h2>
        <p className="text-muted">{t("meetings.jev.onboardingConsent")}</p>
      </div>

      <div className="max-w-xl w-full flex flex-col gap-3">
        <div className="flex flex-col gap-1 text-sm">
          <span id="assistant-provider-label">
            {t("settings.assistant.provider.select.title")}
          </span>
          <div role="group" aria-labelledby="assistant-provider-label">
            <ProviderSelect
              options={state.providerOptions}
              value={state.selectedProviderId}
              onChange={state.handleProviderSelect}
            />
          </div>
        </div>
        <>
          {state.isCustomProvider && (
            <label className="flex flex-col gap-1 text-sm">
              {t("settings.assistant.provider.baseUrl.title")}
              <BaseUrlField
                value={state.baseUrl}
                onBlur={state.handleBaseUrlChange}
                disabled={state.isBaseUrlUpdating}
                placeholder={t(
                  "settings.assistant.provider.baseUrl.placeholder",
                )}
                className="min-w-0"
              />
            </label>
          )}
          <label className="flex flex-col gap-1 text-sm">
            {t("settings.assistant.provider.apiKey.title")}
            <ApiKeyField
              value={state.apiKey}
              onBlur={state.handleApiKeyChange}
              disabled={state.isApiKeyUpdating}
              placeholder={t(
                state.usesBundledKey
                  ? "settings.assistant.provider.apiKey.bundledPlaceholder"
                  : "settings.assistant.provider.apiKey.placeholder",
              )}
              className="min-w-0"
            />
          </label>
          <label className="flex flex-col gap-1 text-sm">
            {t("settings.assistant.provider.model.title")}
            <ModelSelect
              value={state.model}
              options={state.modelOptions}
              disabled={state.isModelUpdating}
              isLoading={state.isFetchingModels}
              onSelect={state.handleModelSelect}
              onCreate={state.handleModelCreate}
              onBlur={() => {}}
              className="min-w-0"
            />
          </label>
        </>
        <ModelPingButton size="sm" onResult={setPinged} />
        <p className="text-xs text-faint">{t("onboarding.assistant.hint")}</p>
      </div>

      <div className="flex gap-3">
        <Button variant="ghost" onClick={onComplete}>
          {t("onboarding.assistant.skip")}
        </Button>
        <Button onClick={onComplete} disabled={!canContinue}>
          {t("onboarding.assistant.continue")}
        </Button>
      </div>
    </div>
  );
};

export default AssistantOnboarding;
