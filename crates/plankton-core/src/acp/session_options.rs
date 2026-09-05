use super::*;
use serde::Deserialize;

/// UI contract flattened from the agent's current, ordered capability catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpConfigValue {
    pub value: String,
    pub name: String,
    pub description: Option<String>,
    pub group: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpConfigOption {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub current_value: String,
    pub options: Vec<AcpConfigValue>,
}

pub(super) struct ConfiguredSession {
    pub options: Vec<AcpConfigOption>,
    pub rejected: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireSession {
    #[serde(default)]
    config_options: Vec<acp::SessionConfigOption>,
    models: Option<acp::SessionModelState>,
    modes: Option<acp::SessionModeState>,
}

fn value(option: acp::SessionConfigSelectOption, group: Option<String>) -> AcpConfigValue {
    AcpConfigValue {
        value: option.value.to_string(),
        name: option.name,
        description: option.description,
        group,
    }
}

pub(super) fn session_options<T: Serialize>(
    session: &T,
) -> Result<Vec<AcpConfigOption>, ProviderError> {
    let session: WireSession = serde_json::from_value(
        serde_json::to_value(session).map_err(|error| ProviderError::Config(error.to_string()))?,
    )
    .map_err(|error| ProviderError::Config(error.to_string()))?;
    let mut options: Vec<_> = session
        .config_options
        .into_iter()
        .filter_map(|option| {
            let acp::SessionConfigKind::Select(select) = option.kind else {
                return None;
            };
            let values = match select.options {
                acp::SessionConfigSelectOptions::Ungrouped(values) => {
                    values.into_iter().map(|v| value(v, None)).collect()
                }
                acp::SessionConfigSelectOptions::Grouped(groups) => groups
                    .into_iter()
                    .flat_map(|g| {
                        g.options
                            .into_iter()
                            .map(move |v| value(v, Some(g.name.clone())))
                    })
                    .collect(),
                _ => return None,
            };
            Some(AcpConfigOption {
                id: option.id.to_string(),
                name: option.name,
                description: option.description,
                category: option
                    .category
                    .and_then(|c| serde_json::to_value(c).ok())
                    .and_then(|v| v.as_str().map(str::to_string)),
                current_value: select.current_value.to_string(),
                options: values,
            })
        })
        .collect();
    // Older adapters advertise only models/modes. Prefer configOptions whenever present.
    if options.is_empty() {
        if let Some(models) = session.models {
            options.push(AcpConfigOption {
                id: "legacy:model".into(),
                name: "Model".into(),
                category: Some("model".into()),
                description: None,
                current_value: models.current_model_id.to_string(),
                options: models
                    .available_models
                    .into_iter()
                    .map(|m| AcpConfigValue {
                        value: m.model_id.to_string(),
                        name: m.name,
                        description: m.description,
                        group: None,
                    })
                    .collect(),
            });
        }
        if let Some(modes) = session.modes {
            options.push(AcpConfigOption {
                id: "legacy:mode".into(),
                name: "Mode".into(),
                category: Some("mode".into()),
                description: None,
                current_value: modes.current_mode_id.to_string(),
                options: modes
                    .available_modes
                    .into_iter()
                    .map(|m| AcpConfigValue {
                        value: m.id.to_string(),
                        name: m.name,
                        description: m.description,
                        group: None,
                    })
                    .collect(),
            });
        }
    }
    Ok(options)
}

async fn set_option(
    config: &AcpSessionConfig,
    conn: &acp::ClientSideConnection,
    session_id: &acp::SessionId,
    option: &AcpConfigOption,
    selected: &str,
) -> Result<Option<Vec<AcpConfigOption>>, ProviderError> {
    let result = timeout(config.timeout, async {
        match option.id.as_str() {
            "legacy:model" => {
                conn.set_session_model(acp::SetSessionModelRequest::new(
                    session_id.clone(),
                    selected.to_string(),
                ))
                .await?;
                Ok(None)
            }
            "legacy:mode" => {
                conn.set_session_mode(acp::SetSessionModeRequest::new(
                    session_id.clone(),
                    selected.to_string(),
                ))
                .await?;
                Ok(None)
            }
            id => conn
                .set_session_config_option(acp::SetSessionConfigOptionRequest::new(
                    session_id.clone(),
                    id.to_string(),
                    selected.to_string(),
                ))
                .await
                .map(Some),
        }
    })
    .await
    .map_err(|_| ProviderError::Transport(format!("ACP configuration timed out: {}", option.id)))?
    .map_err(|error| {
        ProviderError::Transport(format!("ACP configuration failed ({}): {error}", option.id))
    })?;
    result
        .map(|response| session_options(&response))
        .transpose()
}

pub(super) async fn configure_session<T: Serialize>(
    config: &AcpSessionConfig,
    conn: &acp::ClientSideConnection,
    session_id: &acp::SessionId,
    session: &T,
    strict: bool,
) -> Result<ConfiguredSession, ProviderError> {
    let mut options = session_options(session)?;
    let mut desired = config.session_options.clone();
    // Permission policy is independent of model choices. Only select a mode the agent advertised.
    if let Some(option) = options
        .iter()
        .find(|o| o.category.as_deref() == Some("mode") || o.id == "mode")
    {
        if let Some(full) = ["agent-full-access", "bypassPermissions", "full-access"]
            .into_iter()
            .find(|v| option.options.iter().any(|o| o.value == *v))
        {
            desired
                .entry(option.id.clone())
                .or_insert_with(|| full.into());
        }
    }
    // Models can change effort/fast catalogs. Apply model selectors before dependent options.
    let mut ids: Vec<_> = desired.keys().cloned().collect();
    ids.sort_by_key(|id| {
        usize::from(
            !options.iter().any(|o| {
                &o.id == id && (o.category.as_deref() == Some("model") || o.id == "model")
            }),
        )
    });
    let mut rejected = Vec::new();
    for id in ids {
        let selected = &desired[&id];
        let supported = options
            .iter()
            .find(|o| o.id == id && o.options.iter().any(|v| &v.value == selected))
            .cloned();
        let Some(option) = supported else {
            if strict {
                return Err(ProviderError::Config(format!(
                    "ACP no longer offers {id}={selected}; refresh the agent options"
                )));
            }
            rejected.push(id);
            continue;
        };
        if option.current_value == *selected {
            continue;
        }
        if let Some(updated) = set_option(config, conn, session_id, &option, selected).await? {
            if !updated
                .iter()
                .any(|o| o.id == id && o.current_value == *selected)
            {
                return Err(ProviderError::Config(format!(
                    "ACP did not confirm {id}={selected}"
                )));
            }
            options = updated;
        } else if let Some(option) = options.iter_mut().find(|o| o.id == id) {
            option.current_value = selected.clone();
        }
    }
    // A later selection must not silently reset an earlier user choice.
    for (id, selected) in &config.session_options {
        if !rejected.contains(id)
            && !options
                .iter()
                .any(|o| &o.id == id && &o.current_value == selected)
        {
            if strict {
                return Err(ProviderError::Config(format!(
                    "ACP reset configured value {id}={selected}"
                )));
            }
            rejected.push(id.clone());
        }
    }
    Ok(ConfiguredSession { options, rejected })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_agent_order_groups_and_unknown_categories() {
        let options = session_options(&json!({"configOptions":[
            {"id":"speed","name":"Speed","category":"_vendor_speed","type":"select","currentValue":"turbo","options":[{"group":"speeds","name":"Available","options":[{"value":"turbo","name":"Turbo"}]}]},
            {"id":"model","name":"Model","type":"select","currentValue":"new-model","options":[{"value":"new-model","name":"New model"}]}
        ]})).unwrap();
        assert_eq!(options[0].id, "speed");
        assert_eq!(options[0].options[0].group.as_deref(), Some("Available"));
        assert_eq!(options[0].category.as_deref(), Some("_vendor_speed"));
        assert_eq!(options[1].current_value, "new-model");
    }

    #[test]
    fn uses_legacy_model_and_mode_catalogs_only_when_config_options_absent() {
        let mut response = json!({
            "models":{"currentModelId":"opus","availableModels":[{"modelId":"opus","name":"Opus"}]},
            "modes":{"currentModeId":"default","availableModes":[{"id":"default","name":"Default"}]}
        });
        let options = session_options(&response).unwrap();
        assert_eq!(
            options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
            ["legacy:model", "legacy:mode"]
        );
        response["configOptions"] = json!([{"id":"model","name":"Current","type":"select","currentValue":"other","options":[{"value":"other","name":"Other"}]}]);
        let options = session_options(&response).unwrap();
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].id, "model");
    }
}
