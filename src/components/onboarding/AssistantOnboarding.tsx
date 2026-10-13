import { useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { Button } from "../ui/Button";
import { ProviderSelect } from "../settings/PostProcessingSettingsApi/ProviderSelect";
import { BaseUrlField } from "../settings/PostProcessingSettingsApi/BaseUrlField";
import { ApiKeyField } from "../settings/PostProcessingSettingsApi/ApiKeyField";
import { ModelSelect } from "../settings/PostProcessingSettingsApi/ModelSelect";
import { usePostProcessProviderState } from "../settings/PostProcessingSettingsApi/usePostProcessProviderState";

interface AssistantOnboardingProps {
  onComplete: () => void;
}

type TestState =
  | { status: "idle" }
  | { status: "testing" }
  | { status: "success"; count: number }
  | { status: "failure"; error: string };

export const AssistantOnboarding: React.FC<AssistantOnboardingProps> = ({
  onComplete,
}) => {
  const { t } = useTranslation();
  const state = usePostProcessProviderState();
  const [test, setTest] = useState<TestState>({ status: "idle" });

  const runTest = async () => {
    setTest({ status: "testing" });
    try {
      const result = await commands.fetchPostProcessModels(
        state.selectedProviderId,
      );
      if (result.status === "ok") {
        setTest({ status: "success", count: result.data.length });
      } else {
        setTest({ status: "failure", error: result.error });
      }
    } catch (e) {
      setTest({
        status: "failure",
        error: e instanceof Error ? e.message : String(e),
      });
    }
  };

  const canContinue = Boolean(state.model) || test.status === "success";

  return (
    <div className="h-screen w-screen flex flex-col items-center justify-center p-6 gap-6">
      <div className="max-w-xl w-full flex flex-col gap-2 text-center">
        <h2 className="text-xl font-semibold">
          {t("onboarding.assistant.title")}
        </h2>
        <p className="text-text/70">{t("onboarding.assistant.description")}</p>
      </div>

      <div className="max-w-xl w-full flex flex-col gap-3">
        <label className="flex flex-col gap-1 text-sm">
          {t("settings.postProcessing.api.provider.title")}
          <ProviderSelect
            options={state.providerOptions}
            value={state.selectedProviderId}
            onChange={state.handleProviderSelect}
          />
        </label>
        {state.isAppleProvider ? (
          <p className="text-sm text-red-500">
            {t("onboarding.assistant.unsupported")}
          </p>
        ) : (
          <>
            {state.isCustomProvider && (
              <label className="flex flex-col gap-1 text-sm">
                {t("settings.postProcessing.api.baseUrl.title")}
                <BaseUrlField
                  value={state.baseUrl}
                  onBlur={state.handleBaseUrlChange}
                  disabled={state.isBaseUrlUpdating}
                  placeholder={t(
                    "settings.postProcessing.api.baseUrl.placeholder",
                  )}
                  className="min-w-0"
                />
              </label>
            )}
            <label className="flex flex-col gap-1 text-sm">
              {t("settings.postProcessing.api.apiKey.title")}
              <ApiKeyField
                value={state.apiKey}
                onBlur={state.handleApiKeyChange}
                disabled={state.isApiKeyUpdating}
                placeholder={t(
                  "settings.postProcessing.api.apiKey.placeholder",
                )}
                className="min-w-0"
              />
            </label>
            <label className="flex flex-col gap-1 text-sm">
              {t("settings.postProcessing.api.model.title")}
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
        )}
        <div className="flex items-center gap-3">
          <Button
            variant="secondary"
            size="sm"
            onClick={runTest}
            disabled={test.status === "testing" || state.isAppleProvider}
          >
            {test.status === "testing"
              ? t("onboarding.assistant.testing")
              : t("onboarding.assistant.test")}
          </Button>
          <div role="status" aria-live="polite">
            {test.status === "success" && (
              <span className="text-sm text-green-500">
                {t("onboarding.assistant.success", { count: test.count })}
              </span>
            )}
            {test.status === "failure" && (
              <span className="text-sm text-red-500">
                {t("onboarding.assistant.failure", { error: test.error })}
              </span>
            )}
          </div>
        </div>
        <p className="text-xs text-text/50">{t("onboarding.assistant.hint")}</p>
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
