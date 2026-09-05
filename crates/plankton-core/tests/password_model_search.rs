use std::collections::BTreeMap;

use plankton_core::passwords::{Field, FieldKind, Item, ItemCategory, Section, Vault, VaultGroup};
use plankton_core::resources::{
    search::{ResourceSearchEngine, SearchQuery},
    BackendKind, ResourceDocument,
};

fn github_item(id: &str, title: &str, notes: &str, tags: &[&str]) -> Item {
    Item {
        id: id.into(),
        title: title.into(),
        category: ItemCategory::ApiCredential,
        sections: vec![Section {
            id: format!("{id}-credentials"),
            title: "API Credentials".into(),
            fields: vec![Field {
                id: format!("{id}-token"),
                key: "api_token".into(),
                label: "Personal Access Token".into(),
                kind: FieldKind::Concealed,
                value: "not-indexed".into(),
            }],
        }],
        tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        aliases: vec!["gh".into()],
        notes: notes.into(),
        metadata: BTreeMap::from([("environment".into(), "production".into())]),
        archived: false,
    }
}

#[test]
fn vault_model_preserves_hierarchy_and_field_identity() {
    let item = github_item(
        "item-1",
        "GitHub Production",
        "release automation",
        &["prod"],
    );
    let vault = Vault {
        id: "vault-1".into(),
        name: "Engineering".into(),
        groups: vec![VaultGroup {
            id: "group-1".into(),
            name: "Infrastructure".into(),
            parent_id: None,
        }],
        items: vec![item.clone()],
    };

    vault.validate().expect("valid hierarchy");
    assert_eq!(
        item.resource_uri("item-1-token"),
        Some("plankton://field/item-1/item-1-token".into())
    );
    assert_eq!(vault.items[0].sections[0].fields[0].value, "not-indexed");
}

#[test]
fn vault_model_rejects_duplicate_field_keys_across_sections() {
    let mut item = github_item("item-1", "GitHub", "", &[]);
    item.sections.push(Section {
        id: "extra".into(),
        title: "Extra".into(),
        fields: vec![Field {
            id: "duplicate".into(),
            key: "API_TOKEN".into(),
            label: "Duplicate".into(),
            kind: FieldKind::Concealed,
            value: "duplicate".into(),
        }],
    });
    let vault = Vault {
        id: "vault-1".into(),
        name: "Engineering".into(),
        groups: vec![],
        items: vec![item],
    };

    assert!(vault.validate().is_err());
}

#[test]
fn search_matches_notes_field_keys_labels_tags_aliases_cjk_and_typos() {
    let documents = vec![
        ResourceDocument::from_item(
            BackendKind::Local,
            "binding-local",
            "vault-local",
            &github_item(
                "local-github",
                "GitHub 发布令牌",
                "用于生产环境的 release automation",
                &["生产", "scm"],
            ),
        ),
        ResourceDocument::from_item(
            BackendKind::OnePassword,
            "binding-op",
            "vault-op",
            &github_item("op-gitlab", "GitLab", "staging only", &["staging"]),
        ),
    ];
    let engine = ResourceSearchEngine::new(documents, 9);

    for query in [
        "发布",
        "release",
        "api_token",
        "Personal Access",
        "生产",
        "gh",
    ] {
        let results = engine
            .search(&SearchQuery::new(query))
            .expect("search should succeed");
        assert_eq!(
            results.items[0].resource_id, "plankton://field/local-github/local-github-token",
            "query {query}"
        );
    }

    let typo_results = engine
        .search(&SearchQuery::new("GitHib"))
        .expect("fuzzy search should succeed");
    assert_eq!(typo_results.items[0].display_name, "GitHub 发布令牌");
}

#[test]
fn search_filters_tags_and_field_key_with_stable_pagination() {
    let documents = ["a", "b", "c"]
        .into_iter()
        .map(|id| {
            ResourceDocument::from_item(
                BackendKind::Local,
                "local",
                "vault",
                &github_item(id, &format!("GitHub {id}"), "", &["prod", "scm"]),
            )
        })
        .collect();
    let engine = ResourceSearchEngine::new(documents, 42);
    let query = SearchQuery {
        text: "github".into(),
        tag_all: vec!["PROD".into(), "scm".into()],
        tag_any: vec![],
        field_key: Some("API_TOK".into()),
        notes: None,
        limit: 2,
        cursor: None,
    };

    let first = engine.search(&query).expect("first page");
    assert_eq!(first.items.len(), 2);
    assert!(first.next_cursor.is_some());
    let second = engine
        .search(&SearchQuery {
            cursor: first.next_cursor,
            ..query
        })
        .expect("second page");
    assert_eq!(second.items.len(), 1);
    assert_ne!(first.items[0].resource_id, second.items[0].resource_id);
}

#[test]
fn merged_search_projection_never_exposes_backend_identity() {
    let documents = vec![ResourceDocument::from_item(
        BackendKind::OnePassword,
        "hidden-binding",
        "hidden-vault",
        &github_item("op-github", "GitHub", "", &["prod"]),
    )];
    let engine = ResourceSearchEngine::new(documents, 1);
    let result = engine
        .search(&SearchQuery::new("github"))
        .expect("search should work");
    let json = serde_json::to_string(&result).expect("serialize AI projection");

    assert!(!json.contains("one_password"));
    assert!(!json.contains("hidden-binding"));
    assert!(!json.contains("hidden-vault"));
}
