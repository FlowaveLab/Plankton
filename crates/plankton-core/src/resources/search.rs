use std::cmp::Reverse;

use plankton_protocol::resources::{
    MatchField, ResourceSearchItem, ResourceSearchResponse, ResourceSearchWarning,
};
use serde::{Deserialize, Serialize};
use strsim::jaro_winkler;
use unicode_normalization::UnicodeNormalization;

use super::ResourceDocument;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSearchEngine {
    documents: Vec<ResourceDocument>,
    generation: u64,
}

impl ResourceSearchEngine {
    pub fn new(documents: Vec<ResourceDocument>, generation: u64) -> Self {
        Self {
            documents,
            generation,
        }
    }

    pub fn search(&self, query: &SearchQuery) -> Result<ResourceSearchResponse, SearchError> {
        if query.limit == 0 || query.limit > 200 {
            return Err(SearchError::InvalidLimit(query.limit));
        }
        let offset = decode_cursor(query.cursor.as_deref(), self.generation)?;
        let text = normalize(&query.text);
        let field_filter = query.field_key.as_deref().map(normalize);
        let notes_filter = query.notes.as_deref().map(normalize);
        let tag_all = query
            .tag_all
            .iter()
            .map(|tag| normalize(tag))
            .collect::<Vec<_>>();
        let tag_any = query
            .tag_any
            .iter()
            .map(|tag| normalize(tag))
            .collect::<Vec<_>>();

        let mut scored = self
            .documents
            .iter()
            .filter_map(|document| {
                score_document(
                    document,
                    &text,
                    &tag_all,
                    &tag_any,
                    field_filter.as_deref(),
                    notes_filter.as_deref(),
                )
            })
            .collect::<Vec<_>>();
        scored.sort_by_key(|(score, item)| (Reverse(*score), item.resource_id.clone()));

        let total = scored.len();
        let items = scored
            .into_iter()
            .skip(offset)
            .take(usize::from(query.limit))
            .map(|(_, item)| item)
            .collect::<Vec<_>>();
        let next_offset = offset + items.len();
        let next_cursor =
            (next_offset < total).then(|| format!("{}:{next_offset}", self.generation));

        Ok(ResourceSearchResponse {
            items,
            next_cursor,
            index_generation: self.generation,
            warnings: Vec::<ResourceSearchWarning>::new(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchQuery {
    pub text: String,
    #[serde(default)]
    pub tag_all: Vec<String>,
    #[serde(default)]
    pub tag_any: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tag_all: vec![],
            tag_any: vec![],
            field_key: None,
            notes: None,
            limit: default_limit(),
            cursor: None,
        }
    }
}

fn default_limit() -> u16 {
    50
}

fn score_document(
    document: &ResourceDocument,
    query: &str,
    tag_all: &[String],
    tag_any: &[String],
    field_filter: Option<&str>,
    notes_filter: Option<&str>,
) -> Option<(u32, ResourceSearchItem)> {
    let normalized_tags = document
        .tags
        .iter()
        .map(|tag| normalize(tag))
        .collect::<Vec<_>>();
    if !tag_all.iter().all(|tag| normalized_tags.contains(tag)) {
        return None;
    }
    if !tag_any.is_empty() && !tag_any.iter().any(|tag| normalized_tags.contains(tag)) {
        return None;
    }
    let normalized_field_key = normalize(&document.field_key);
    if field_filter.is_some_and(|filter| fuzzy_score(&normalized_field_key, filter).is_none()) {
        return None;
    }
    let normalized_notes = normalize(&document.notes);
    if notes_filter.is_some_and(|filter| fuzzy_score(&normalized_notes, filter).is_none()) {
        return None;
    }

    let fields = [
        (
            MatchField::DisplayName,
            normalize(&document.display_name),
            180,
        ),
        (MatchField::FieldKey, normalized_field_key, 170),
        (
            MatchField::FieldLabel,
            normalize(&document.field_label),
            155,
        ),
        (
            MatchField::Alias,
            normalize(&document.aliases.join(" ")),
            145,
        ),
        (MatchField::Tag, normalized_tags.join(" "), 140),
        (MatchField::Note, normalized_notes, 120),
        (MatchField::Section, normalize(&document.section), 110),
        (
            MatchField::Metadata,
            normalize(
                &document
                    .metadata
                    .iter()
                    .map(|(key, value)| format!("{key} {value}"))
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            90,
        ),
    ];

    let mut matched_on = Vec::new();
    let mut score = if query.is_empty() { 1 } else { 0 };
    for (field, candidate, weight) in fields {
        if let Some(field_score) = fuzzy_score(&candidate, query) {
            matched_on.push(field);
            score += field_score * weight / 1000;
        }
    }
    if score == 0 {
        return None;
    }

    Some((
        score,
        ResourceSearchItem {
            resource_id: document.resource_id.clone(),
            display_name: document.display_name.clone(),
            aliases: document.aliases.clone(),
            description: document.description.clone(),
            tags: document.tags.clone(),
            field_key: document.field_key.clone(),
            field_label: document.field_label.clone(),
            matched_on,
            highlights: if query.is_empty() {
                Vec::new()
            } else {
                vec![query.to_string()]
            },
            score,
        },
    ))
}

fn fuzzy_score(candidate: &str, query: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(1000);
    }
    if candidate == query {
        return Some(1000);
    }
    if candidate.contains(query) {
        let density = (query.chars().count() * 200 / candidate.chars().count().max(1)) as u32;
        return Some(750 + density.min(200));
    }
    let best = candidate
        .split_whitespace()
        .chain(std::iter::once(candidate))
        .map(|part| jaro_winkler(part, query))
        .fold(0.0_f64, f64::max);
    (best >= 0.82).then_some((best * 700.0) as u32)
}

fn normalize(value: &str) -> String {
    value.nfkc().collect::<String>().trim().to_lowercase()
}

fn decode_cursor(cursor: Option<&str>, generation: u64) -> Result<usize, SearchError> {
    let Some(cursor) = cursor else {
        return Ok(0);
    };
    let (cursor_generation, offset) = cursor.split_once(':').ok_or(SearchError::InvalidCursor)?;
    let cursor_generation = cursor_generation
        .parse::<u64>()
        .map_err(|_| SearchError::InvalidCursor)?;
    if cursor_generation != generation {
        return Err(SearchError::StaleCursor {
            expected_generation: generation,
            received_generation: cursor_generation,
        });
    }
    offset.parse().map_err(|_| SearchError::InvalidCursor)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SearchError {
    #[error("search limit must be between 1 and 200, received {0}")]
    InvalidLimit(u16),
    #[error("invalid search cursor")]
    InvalidCursor,
    #[error(
        "stale search cursor: expected generation {expected_generation}, received {received_generation}"
    )]
    StaleCursor {
        expected_generation: u64,
        received_generation: u64,
    },
}
