use std::{fmt, sync::Arc};

use plankton_core::{load_settings, PlanktonSettings, SettingsError};

type SettingsLoader = dyn Fn() -> Result<PlanktonSettings, SettingsError> + Send + Sync + 'static;

#[derive(Clone)]
pub(crate) struct RuntimeSettings {
    loader: Arc<SettingsLoader>,
}

impl RuntimeSettings {
    pub(crate) fn reloading() -> Self {
        Self::from_loader(load_settings)
    }

    pub(crate) fn fixed(settings: PlanktonSettings) -> Self {
        Self::from_loader(move || Ok(settings.clone()))
    }

    pub(crate) fn current(&self) -> Result<PlanktonSettings, SettingsError> {
        (self.loader)()
    }

    fn from_loader(
        loader: impl Fn() -> Result<PlanktonSettings, SettingsError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            loader: Arc::new(loader),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_loader(
        loader: impl Fn() -> Result<PlanktonSettings, SettingsError> + Send + Sync + 'static,
    ) -> Self {
        Self::from_loader(loader)
    }
}

impl fmt::Debug for RuntimeSettings {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeSettings")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use plankton_core::PlanktonSettings;

    use super::RuntimeSettings;

    #[test]
    fn loader_reads_the_latest_settings_on_every_access() {
        let settings = Arc::new(Mutex::new(PlanktonSettings {
            provider_kind: "mock".into(),
            ..PlanktonSettings::default()
        }));
        let source = settings.clone();
        let runtime = RuntimeSettings::test_loader(move || {
            Ok(source.lock().expect("settings mutex").clone())
        });

        assert_eq!(
            runtime.current().expect("initial settings").provider_kind,
            "mock"
        );
        settings.lock().expect("settings mutex").provider_kind = "acp".into();
        assert_eq!(
            runtime.current().expect("updated settings").provider_kind,
            "acp"
        );
    }
}
