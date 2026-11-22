//! Reads legacy snapshots and saves typed single-screen prototype documents in app data.

use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

use super::{Error, Starter, repository, validate};

// Keep the version 2 snapshot envelope separate from the public version 1 starter.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Variant {
    id: Option<Uuid>,
    name: String,
    source_url: Option<String>,
    document: Document,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    blocks: Vec<Block>,
}

// Typed, data-only blocks keep each component's editable fields explicit on disk.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Block {
    Heading {
        id: String,
        text: String,
    },
    Text {
        id: String,
        text: String,
    },
    Card {
        id: String,
        title: String,
        body: String,
    },
    Button {
        id: String,
        label: String,
    },
}

// Existing snapshots stay readable and retain their original shape until saved explicitly.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LegacyVariant {
    id: Option<Uuid>,
    name: String,
    source_url: String,
    starter: Starter,
}

#[derive(Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum StoredVariant {
    Current(Variant),
    Legacy(LegacyVariant),
}

impl StoredVariant {
    // Preserve the original name and ID ordering across both formats.
    fn sort_key(&self) -> (&str, Option<Uuid>) {
        match self {
            Self::Current(variant) => (&variant.name, variant.id),
            Self::Legacy(variant) => (&variant.name, variant.id),
        }
    }
}

// Empty documents are valid. Required labels and unique IDs keep blocks selectable.
fn validate_document(document: &Document) -> Result<(), Error> {
    let mut ids = HashSet::new();
    if document.version != 2 {
        return Err(Error::InvalidDocument);
    }
    for block in &document.blocks {
        let (id, has_required_content) = match block {
            Block::Heading { id, text } => (id, !text.trim().is_empty()),
            Block::Text { id, .. } => (id, true),
            Block::Card { id, title, .. } => (id, !title.trim().is_empty()),
            Block::Button { id, label } => (id, !label.trim().is_empty()),
        };
        if id.trim().is_empty() || !ids.insert(id) || !has_required_content {
            return Err(Error::InvalidDocument);
        }
    }
    Ok(())
}

// Use the same app-owned location so existing variants remain visible.
pub(super) fn directory(app: &AppHandle) -> Result<PathBuf, Error> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("prototype-builder"))
        .map_err(|_| Error::StorageUnavailable)
}

// Validate before touching disk and flush the staged file before replacing a snapshot.
pub(super) fn save(dir: &Path, mut variant: Variant) -> Result<Uuid, Error> {
    validate_document(&variant.document)?;
    if variant.name.trim().is_empty() {
        return Err(Error::InvalidName);
    }
    variant.name = variant.name.trim().to_owned();
    if let Some(source) = &variant.source_url {
        variant.source_url = Some(repository(source)?.0);
    }
    let id = *variant.id.get_or_insert_with(Uuid::new_v4);
    fs::create_dir_all(dir).map_err(|_| Error::StorageUnavailable)?;
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|_| Error::StorageUnavailable)?;
    serde_json::to_writer(&mut file, &variant).map_err(|_| Error::StorageUnavailable)?;
    file.flush().map_err(|_| Error::StorageUnavailable)?;
    file.as_file()
        .sync_all()
        .map_err(|_| Error::StorageUnavailable)?;
    file.persist(dir.join(format!("{id}.json")))
        .map_err(|_| Error::StorageUnavailable)?;
    Ok(id)
}

// Load both snapshot formats without fetching sources or writing migrations to disk.
pub(super) fn list(dir: &Path) -> Result<Vec<StoredVariant>, Error> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(Error::StorageUnavailable),
    };
    let mut variants = Vec::new();
    for entry in entries {
        let path = entry.map_err(|_| Error::StorageUnavailable)?.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let file = fs::File::open(path).map_err(|_| Error::StorageUnavailable)?;
        let variant = serde_json::from_reader(file).map_err(|_| Error::StorageUnavailable)?;
        match &variant {
            StoredVariant::Current(variant) => validate_document(&variant.document)?,
            StoredVariant::Legacy(variant) => validate(&variant.starter)?,
        }
        variants.push(variant);
    }
    variants.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
    Ok(variants)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    // Use the IPC shape in tests so the field and block tags stay in sync with Vue.
    fn make_variant(id: Option<Uuid>, name: &str, blocks: Value) -> Variant {
        serde_json::from_value(json!({
            "id": id,
            "name": name,
            "sourceUrl": null,
            "document": { "version": 2, "blocks": blocks }
        }))
        .unwrap()
    }

    // An old file survives opening and coexists with a blank, source-free document.
    #[test]
    fn reopens_mixed_versions_and_upgrades_only_on_save() {
        let dir = tempfile::tempdir().unwrap();
        let legacy_id = Uuid::new_v4();
        let path = dir.path().join(format!("{legacy_id}.json"));
        let starter: Value = serde_json::from_str(include_str!(
            "../../../examples/prototype-builder/wits-starter.json"
        ))
        .unwrap();
        let legacy = json!({
            "id": legacy_id,
            "name": "Legacy variant",
            "sourceUrl": "https://github.com/owner/repo",
            "starter": starter
        });
        let original = serde_json::to_vec(&legacy).unwrap();
        fs::write(&path, &original).unwrap();
        let blank_id = save(dir.path(), make_variant(None, "Blank", json!([]))).unwrap();

        let variants = list(dir.path()).unwrap();
        let blank = serde_json::to_value(&variants[0]).unwrap();
        assert_eq!(blank["id"], json!(blank_id));
        assert_eq!(blank["sourceUrl"], Value::Null);
        assert_eq!(blank["document"]["blocks"], json!([]));
        assert_eq!(serde_json::to_value(&variants[1]).unwrap(), legacy);
        assert_eq!(fs::read(&path).unwrap(), original);

        let mut upgraded = make_variant(Some(legacy_id), "Legacy variant", json!([]));
        upgraded.source_url = Some("https://github.com/owner/repo/".to_owned());
        assert_eq!(save(dir.path(), upgraded).unwrap(), legacy_id);
        let variants = list(dir.path()).unwrap();
        let upgraded = serde_json::to_value(&variants[1]).unwrap();
        assert_eq!(upgraded["id"], json!(legacy_id));
        assert_eq!(upgraded["sourceUrl"], "https://github.com/owner/repo");
        assert_eq!(upgraded["document"]["version"], 2);
        assert!(upgraded.get("starter").is_none());
    }

    // Saving as new allocates an ID and overwriting a separate copy cannot change the original.
    #[test]
    fn saves_and_reopens_independent_variants_with_all_block_types() {
        let dir = tempfile::tempdir().unwrap();
        let blocks = json!([
            { "id": "heading", "kind": "heading", "text": "Release readiness" },
            { "id": "text", "kind": "text", "text": "Project status" },
            { "id": "card", "kind": "card", "title": "QA", "body": "Passed" },
            { "id": "button", "kind": "button", "label": "See status" }
        ]);
        let first = save(dir.path(), make_variant(None, "First", blocks.clone())).unwrap();
        let second = save(dir.path(), make_variant(None, "Second", blocks.clone())).unwrap();
        let mut revised = blocks.clone();
        revised[0]["text"] = json!("Revised title");
        assert_eq!(
            save(
                dir.path(),
                make_variant(Some(second), "Second", revised.clone())
            )
            .unwrap(),
            second
        );
        let variants = list(dir.path()).unwrap();
        let original = serde_json::to_value(&variants[0]).unwrap();
        let copy = serde_json::to_value(&variants[1]).unwrap();
        assert_eq!(original["id"], json!(first));
        assert_eq!(original["document"]["blocks"], blocks);
        assert_eq!(copy["id"], json!(second));
        assert_eq!(copy["document"]["blocks"], revised);
    }

    // A failed save leaves the previous on-disk snapshot available.
    #[test]
    fn rejects_invalid_documents_before_replacing_a_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let id = save(dir.path(), make_variant(None, "Keep me", json!([]))).unwrap();
        let duplicate = json!([
            { "id": "reused", "kind": "text", "text": "" },
            { "id": "reused", "kind": "card", "title": "Title", "body": "" }
        ]);
        let blank_label = json!([{ "id": "button", "kind": "button", "label": " " }]);

        for blocks in [duplicate, blank_label] {
            let error = save(
                dir.path(),
                make_variant(Some(id), "Bad replacement", blocks),
            );
            assert!(matches!(error, Err(Error::InvalidDocument)));
        }

        let variants = list(dir.path()).unwrap();
        assert_eq!(variants[0].sort_key(), ("Keep me", Some(id)));
    }
}
