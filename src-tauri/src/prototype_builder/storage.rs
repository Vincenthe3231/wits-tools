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

// Keep local snapshot metadata separate from the public version 1 starter.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Variant {
    id: Option<Uuid>,
    name: String,
    source_url: Option<String>,
    document: Document,
}

// Read both local versions without rewriting old files on open.
#[derive(Deserialize, Serialize)]
#[serde(untagged)]
enum Document {
    Canvas(CanvasDocument),
    Linear(LinearDocument),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LinearDocument {
    version: u8,
    blocks: Vec<Block>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CanvasDocument {
    version: u8,
    screen: Screen,
    nodes: Vec<Node>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Screen {
    width: u32,
    height: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Node {
    block: Block,
    x: i32,
    y: i32,
    width: u32,
    // A missing height preserves intrinsic sizing in older v3 snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
}

impl Variant {
    // Only the new format can be written through the desktop command.
    pub(super) fn require_canvas(&self) -> Result<(), Error> {
        match self.document {
            Document::Canvas(_) => Ok(()),
            Document::Linear(_) => Err(Error::InvalidDocument),
        }
    }
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
    // Match the field names sent by Vue and persist only authored copy.
    #[serde(rename_all = "camelCase")]
    SignIn {
        id: String,
        title: String,
        email_label: String,
        password_label: String,
        button_label: String,
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
    let blocks: Box<dyn Iterator<Item = &Block>> = match document {
        Document::Linear(doc) if doc.version == 2 => Box::new(doc.blocks.iter()),
        Document::Canvas(doc)
            if doc.version == 3
                && valid_dimension(doc.screen.width)
                && valid_dimension(doc.screen.height)
                && doc.nodes.iter().all(|node| {
                    valid_dimension(node.width) && node.height.is_none_or(valid_dimension)
                }) =>
        {
            Box::new(doc.nodes.iter().map(|node| &node.block))
        }
        _ => return Err(Error::InvalidDocument),
    };
    for block in blocks {
        let (id, has_required_content) = match block {
            Block::Heading { id, text } => (id, !text.trim().is_empty()),
            Block::Text { id, .. } => (id, true),
            Block::Card { id, title, .. } => (id, !title.trim().is_empty()),
            Block::Button { id, label } => (id, !label.trim().is_empty()),
            // All visible titles and control labels must remain meaningful on reopen.
            Block::SignIn {
                id,
                title,
                email_label,
                password_label,
                button_label,
            } => (
                id,
                [title, email_label, password_label, button_label]
                    .iter()
                    .all(|text| !text.trim().is_empty()),
            ),
        };
        if id.trim().is_empty() || !ids.insert(id) || !has_required_content {
            return Err(Error::InvalidDocument);
        }
    }
    Ok(())
}

// Cap dimensions within the DOM layout range used by the editor.
fn valid_dimension(value: u32) -> bool {
    (1..=1_000_000).contains(&value)
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
            { "id": "button", "kind": "button", "label": "See status" },
            { "id": "login", "kind": "signIn", "title": "Account access",
              "emailLabel": "Work email", "passwordLabel": "Passphrase", "buttonLabel": "Continue" }
        ]);
        let first = save(dir.path(), make_variant(None, "First", blocks.clone())).unwrap();
        let second = save(dir.path(), make_variant(None, "Second", blocks.clone())).unwrap();
        let mut revised = blocks.clone();
        revised[0]["text"] = json!("Revised title");
        // Save edited copy at a new position and verify both survive reopening.
        revised[4]["buttonLabel"] = json!("Access workspace");
        revised.as_array_mut().unwrap().swap(1, 4);
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

    // Keep geometry, back-to-front order, and older snapshots across independent saves.
    #[test]
    fn saves_canvas_geometry_and_rejects_invalid_replacements() {
        let dir = tempfile::tempdir().unwrap();
        let old_id = save(dir.path(), make_variant(None, "Linear", json!([]))).unwrap();
        let document = json!({
            "version": 3,
            "screen": { "width": 390, "height": 844 },
            "nodes": [
                { "x": -25, "y": 90, "width": 160,
                  "block": { "id": "text", "kind": "text", "text": "Behind" } },
                { "x": 10, "y": 90, "width": 320, "height": 380,
                  "block": { "id": "form", "kind": "signIn", "title": "Welcome",
                    "emailLabel": "Email", "passwordLabel": "Password", "buttonLabel": "Continue" } }
            ]
        });
        let variant = |id, name, document| {
            serde_json::from_value::<Variant>(json!({
                "id": id, "name": name, "sourceUrl": null, "document": document
            }))
            .unwrap()
        };
        let original = variant(None, "Canvas", document.clone());
        assert!(original.require_canvas().is_ok());
        let id = save(dir.path(), original).unwrap();
        let copy = save(dir.path(), variant(None, "Copy", document.clone())).unwrap();
        assert_ne!(id, copy);
        let mut invalid = document.clone();
        invalid["nodes"][0]["width"] = json!(0);
        assert!(matches!(
            save(dir.path(), variant(Some(id), "Canvas", invalid)),
            Err(Error::InvalidDocument)
        ));
        let saved = list(dir.path()).unwrap();
        assert_eq!(saved[0].sort_key(), ("Canvas", Some(id)));
        assert_eq!(saved[1].sort_key(), ("Copy", Some(copy)));
        assert_eq!(saved[2].sort_key(), ("Linear", Some(old_id)));
        assert_eq!(
            serde_json::to_value(&saved[0]).unwrap()["document"],
            document
        );
        assert_eq!(
            serde_json::to_value(&saved[1]).unwrap()["document"],
            document
        );
    }
}
