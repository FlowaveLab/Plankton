use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vault {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub groups: Vec<VaultGroup>,
    #[serde(default)]
    pub items: Vec<Item>,
}

impl Vault {
    pub fn validate(&self) -> Result<(), ModelError> {
        required("vault.id", &self.id)?;
        required("vault.name", &self.name)?;

        let group_ids = unique_ids("group", self.groups.iter().map(|group| group.id.as_str()))?;
        for group in &self.groups {
            required("group.name", &group.name)?;
            if group.parent_id.as_ref() == Some(&group.id) {
                return Err(ModelError::SelfParent(group.id.clone()));
            }
            if let Some(parent_id) = &group.parent_id {
                if !group_ids.contains(parent_id.as_str()) {
                    return Err(ModelError::MissingParent {
                        group_id: group.id.clone(),
                        parent_id: parent_id.clone(),
                    });
                }
            }
        }
        detect_group_cycles(&self.groups)?;

        unique_ids("item", self.items.iter().map(|item| item.id.as_str()))?;
        for item in &self.items {
            item.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultGroup {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub category: ItemCategory,
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub archived: bool,
}

impl Item {
    pub fn validate(&self) -> Result<(), ModelError> {
        required("item.id", &self.id)?;
        required("item.title", &self.title)?;
        unique_ids(
            "section",
            self.sections.iter().map(|section| section.id.as_str()),
        )?;

        let mut field_ids = BTreeSet::new();
        let mut field_keys = BTreeSet::new();
        for section in &self.sections {
            required("section.id", &section.id)?;
            required("section.title", &section.title)?;
            for field in &section.fields {
                field.validate()?;
                if !field_ids.insert(field.id.as_str()) {
                    return Err(ModelError::DuplicateId {
                        kind: "field",
                        value: field.id.clone(),
                    });
                }
                let normalized_key = field.key.trim().to_lowercase();
                if !field_keys.insert(normalized_key) {
                    return Err(ModelError::DuplicateFieldKey(field.key.clone()));
                }
            }
        }
        Ok(())
    }

    pub fn resource_uri(&self, field_id: &str) -> Option<String> {
        self.sections
            .iter()
            .flat_map(|section| section.fields.iter())
            .any(|field| field.id == field_id)
            .then(|| format!("plankton://field/{}/{}", self.id, field_id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemCategory {
    Login,
    ApiCredential,
    Database,
    SecureNote,
    Identity,
    Server,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub id: String,
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    pub value: String,
}

impl Field {
    fn validate(&self) -> Result<(), ModelError> {
        required("field.id", &self.id)?;
        required("field.key", &self.key)?;
        required("field.label", &self.label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Concealed,
    Text,
    Username,
    Password,
    Url,
    Email,
    Otp,
    Date,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("{0} cannot be empty")]
    Empty(&'static str),
    #[error("duplicate {kind} id: {value}")]
    DuplicateId { kind: &'static str, value: String },
    #[error("duplicate field key: {0}")]
    DuplicateFieldKey(String),
    #[error("group {group_id} references missing parent {parent_id}")]
    MissingParent { group_id: String, parent_id: String },
    #[error("group {0} cannot be its own parent")]
    SelfParent(String),
    #[error("group hierarchy contains a cycle at {0}")]
    GroupCycle(String),
}

fn required(field: &'static str, value: &str) -> Result<(), ModelError> {
    if value.trim().is_empty() {
        Err(ModelError::Empty(field))
    } else {
        Ok(())
    }
}

fn unique_ids<'a>(
    kind: &'static str,
    ids: impl Iterator<Item = &'a str>,
) -> Result<BTreeSet<&'a str>, ModelError> {
    let mut unique = BTreeSet::new();
    for id in ids {
        required(kind, id)?;
        if !unique.insert(id) {
            return Err(ModelError::DuplicateId {
                kind,
                value: id.to_string(),
            });
        }
    }
    Ok(unique)
}

fn detect_group_cycles(groups: &[VaultGroup]) -> Result<(), ModelError> {
    let parents = groups
        .iter()
        .map(|group| (group.id.as_str(), group.parent_id.as_deref()))
        .collect::<BTreeMap<_, _>>();
    for group in groups {
        let mut seen = BTreeSet::new();
        let mut current = Some(group.id.as_str());
        while let Some(id) = current {
            if !seen.insert(id) {
                return Err(ModelError::GroupCycle(id.to_string()));
            }
            current = parents.get(id).copied().flatten();
        }
    }
    Ok(())
}
