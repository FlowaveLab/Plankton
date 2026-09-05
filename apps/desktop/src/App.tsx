import { useCallback, useRef, type JSX } from "react";

import { DesktopWorkspace } from "./components/DesktopWorkspace";
import { useDesktopApp } from "./hooks/useDesktopApp";

export default function App(): JSX.Element {
  const {
    state,
    hasUnsavedSettings,
    canSaveSettings,
    settingsValidationMessage,
    dismissError,
    reloadSettings,
    saveSettings,
    setAcpProfile,
    setPolicyMode,
    setProviderKind,
    setLocale,
    setNoteDraft,
    updateSettingsField,
    decide,
  } = useDesktopApp();
  const setNoteDraftRef = useRef(setNoteDraft);
  setNoteDraftRef.current = setNoteDraft;
  const handleNoteChange = useCallback((value: string) => {
    setNoteDraftRef.current(value);
  }, []);

  return (
    <DesktopWorkspace
      dashboard={state.dashboard}
      errorMessage={state.errorMessage}
      focusedRequestId={state.lastHandoffRequestId}
      isSubmitting={state.isSubmitting}
      locale={state.locale}
      noteDraft={state.noteDraft}
      onDecision={decide}
      onDismissError={dismissError}
      onLocaleChange={setLocale}
      onNoteChange={handleNoteChange}
      settingsController={{
        settings: state.settings,
        settingsDraft: state.settingsDraft,
        isLoading: state.isSettingsLoading,
        isSaving: state.isSettingsSaving,
        errorMessage: state.settingsErrorMessage,
        noticeMessage: state.settingsNoticeMessage,
        hasUnsavedChanges: hasUnsavedSettings,
        canSave: canSaveSettings,
        validationMessage: settingsValidationMessage,
        onSave: () => {
          void saveSettings();
        },
        onReload: () => {
          void reloadSettings();
        },
        onPolicyModeChange: setPolicyMode,
        onProviderKindChange: setProviderKind,
        onAcpProfileChange: setAcpProfile,
        onFieldChange: updateSettingsField,
      }}
    />
  );
}
