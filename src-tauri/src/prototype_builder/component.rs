//! Validates and stores the data-only component format used by the palette and saved prototypes.

use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::Mutex,
};

use jsonc_parser::{ast, common::Ranged};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

const MAX_BYTES: u64 = 64 * 1024;
const ROLES: [&str; 7] = [
    "screen", "surface", "text", "muted", "border", "accent", "onAccent",
];
static LIBRARY_LOCK: Mutex<()> = Mutex::new(());

// The same types validate definitions embedded in projects and selected for import.
#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ComponentDefinition {
    version: u8,
    pub(super) id: String,
    pub(super) revision: u32,
    name: String,
    default_width: u32,
    pub(super) fields: BTreeMap<String, Field>,
    root: Element,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Field {
    label: String,
    default: String,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Element {
    Stack {
        children: Vec<Element>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<Style>,
    },
    Row {
        children: Vec<Element>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<Style>,
    },
    Text {
        text: Text,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<Style>,
    },
    Button {
        text: Text,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<Style>,
    },
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
enum Text {
    Literal(Literal),
    Field(FieldRef),
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Literal {
    literal: String,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct FieldRef {
    field: String,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct Style {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    border: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    radius: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    padding: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gap: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "fontSize")]
    font_size: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    weight: Option<u16>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Theme {
    screen: String,
    surface: String,
    text: String,
    muted: String,
    border: String,
    accent: String,
    on_accent: String,
    font: Font,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
enum Font {
    System,
    Serif,
    Mono,
}

fn hex(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}

impl Theme {
    // Colors authored in a project are fixed values independent of system appearance.
    pub(super) fn valid(&self) -> bool {
        [
            &self.screen,
            &self.surface,
            &self.text,
            &self.muted,
            &self.border,
            &self.accent,
            &self.on_accent,
        ]
        .iter()
        .all(|color| hex(color))
    }
}

impl Style {
    // Identify the first invalid value so the importer can report its source location.
    fn invalid_field(&self) -> Option<&'static str> {
        for (key, value) in [
            ("fill", &self.fill),
            ("color", &self.color),
            ("border", &self.border),
        ] {
            if value
                .as_ref()
                .is_some_and(|v| !hex(v) && !ROLES.contains(&v.as_str()))
            {
                return Some(key);
            }
        }
        [
            ("radius", self.radius, 0, 512),
            ("padding", self.padding, 0, 512),
            ("gap", self.gap, 0, 512),
            ("fontSize", self.font_size, 1, 512),
            ("weight", self.weight, 100, 900),
        ]
        .into_iter()
        .find(|(_, value, min, max)| value.is_some_and(|n| n < *min || n > *max))
        .map(|(key, _, _, _)| key)
    }

    pub(super) fn valid(&self) -> bool {
        self.invalid_field().is_none()
    }
}

#[derive(Serialize)]
pub(crate) struct Diagnostic {
    code: &'static str,
    path: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
}

fn issue(code: &'static str, path: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code,
        path: path.to_owned(),
        message: message.into(),
        line: None,
        column: None,
    }
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

impl ComponentDefinition {
    // Collect semantic errors after shape validation so locations map to real keys.
    fn diagnostics(&self) -> Vec<Diagnostic> {
        let mut issues = Vec::new();
        if self.version != 1 {
            issues.push(issue(
                "version.unsupported",
                "/version",
                "Use component format version 1.",
            ));
        }
        if !valid_id(&self.id) {
            issues.push(issue(
                "value.invalidId",
                "/id",
                "Use a qualified ID such as team.pricing-card.",
            ));
        }
        if self.revision == 0 {
            issues.push(issue(
                "value.invalidRevision",
                "/revision",
                "Use a positive revision number.",
            ));
        }
        if self.name.trim().is_empty() || self.name.len() > 100 {
            issues.push(issue(
                "value.invalidName",
                "/name",
                "Enter a name between 1 and 100 bytes.",
            ));
        }
        if !(1..=1_000_000).contains(&self.default_width) {
            issues.push(issue(
                "value.invalidWidth",
                "/defaultWidth",
                "Enter a width from 1 to 1000000.",
            ));
        }
        if self.fields.len() > 16 {
            issues.push(issue(
                "limit.fields",
                "/fields",
                "Declare up to 16 editable fields.",
            ));
        }
        for (key, field) in &self.fields {
            let path = child_path("/fields", key);
            if !identifier(key) {
                issues.push(issue(
                    "value.invalidField",
                    &path,
                    "Use a lowercase field name such as plan-name.",
                ));
            }
            if field.label.trim().is_empty() || field.label.len() > 100 {
                issues.push(issue(
                    "value.invalidLabel",
                    &child_path(&path, "label"),
                    "Enter a label between 1 and 100 bytes.",
                ));
            }
            if field.default.len() > 2000 {
                issues.push(issue(
                    "limit.text",
                    &child_path(&path, "default"),
                    "Keep text within 2000 UTF-8 bytes.",
                ));
            }
        }
        self.validate_element(&self.root, "/root", 1, &mut 0, &mut issues);
        issues
    }

    fn validate_element(
        &self,
        element: &Element,
        path: &str,
        depth: usize,
        count: &mut usize,
        issues: &mut Vec<Diagnostic>,
    ) {
        *count += 1;
        if depth > 6 || *count > 64 {
            if !issues.iter().any(|issue| issue.code == "limit.tree") {
                issues.push(issue(
                    "limit.tree",
                    path,
                    "Use up to 64 elements and 6 levels of nesting.",
                ));
            }
            return;
        }
        let style = match element {
            Element::Stack { children, style } | Element::Row { children, style } => {
                let children_path = child_path(path, "children");
                for (index, child) in children.iter().enumerate() {
                    self.validate_element(
                        child,
                        &child_path(&children_path, &index.to_string()),
                        depth + 1,
                        count,
                        issues,
                    );
                }
                style
            }
            Element::Button { text, style } | Element::Text { text, style } => {
                let text_path = child_path(path, "text");
                match text {
                    Text::Field(reference) if !self.fields.contains_key(&reference.field) => issues
                        .push(issue(
                            "reference.unknownField",
                            &child_path(&text_path, "field"),
                            "Declare this field or use a literal label.",
                        )),
                    Text::Literal(value) if value.literal.len() > 2000 => issues.push(issue(
                        "limit.text",
                        &child_path(&text_path, "literal"),
                        "Keep text within 2000 UTF-8 bytes.",
                    )),
                    _ => {}
                }
                style
            }
        };
        if let Some(key) = style.as_ref().and_then(Style::invalid_field) {
            issues.push(issue("value.invalidStyle", &child_path(&child_path(path, "style"), key),
                "Use a supported color token, six-digit hex color, or a number within the field limits."));
        }
    }

    pub(super) fn valid(&self) -> bool {
        self.diagnostics().is_empty()
    }
}

fn valid_id(id: &str) -> bool {
    id.len() <= 128 && id.contains('.') && id.split('.').all(identifier)
}

fn child_path(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

// Index each value before deserializing so even invalid fields have source positions.
fn index_source(
    value: &ast::Value<'_>,
    path: &str,
    offsets: &mut BTreeMap<String, usize>,
    issues: &mut Vec<(Diagnostic, usize)>,
) {
    offsets.insert(path.to_owned(), value.range().start);
    match value {
        ast::Value::Object(object) => {
            let mut seen = std::collections::HashSet::new();
            for property in &object.properties {
                let key = property.name.as_str();
                let path = child_path(path, key);
                if !seen.insert(key) {
                    issues.push((
                        issue(
                            "structure.duplicateKey",
                            &path,
                            "Remove the repeated property.",
                        ),
                        property.name.range().start,
                    ));
                }
                index_source(&property.value, &path, offsets, issues);
            }
        }
        ast::Value::Array(array) => {
            for (index, value) in array.elements.iter().enumerate() {
                index_source(
                    value,
                    &child_path(path, &index.to_string()),
                    offsets,
                    issues,
                );
            }
        }
        _ => {}
    }
}

fn locate(source: &str, offset: usize, mut diagnostic: Diagnostic) -> Diagnostic {
    let prefix = &source[..offset];
    diagnostic.line = Some(prefix.bytes().filter(|byte| *byte == b'\n').count() + 1);
    diagnostic.column = Some(
        prefix
            .rsplit('\n')
            .next()
            .unwrap_or_default()
            .chars()
            .count()
            + 1,
    );
    diagnostic
}

fn location(source: &str, offsets: &BTreeMap<String, usize>, diagnostic: Diagnostic) -> Diagnostic {
    let mut path = diagnostic.path.as_str();
    loop {
        if let Some(offset) = offsets.get(path) {
            return locate(source, *offset, diagnostic);
        }
        let Some((parent, _)) = path.rsplit_once('/') else {
            return diagnostic;
        };
        path = parent;
    }
}

fn shape_issue(
    error: serde_path_to_error::Error<serde_json::Error>,
    offsets: &BTreeMap<String, usize>,
) -> Diagnostic {
    use serde_path_to_error::Segment;
    let mut path = String::new();
    for segment in error.path() {
        match segment {
            Segment::Seq { index } => path = child_path(&path, &index.to_string()),
            Segment::Map { key } => path = child_path(&path, key),
            _ => {}
        }
    }
    let message = error.inner().to_string();
    if message.starts_with("unknown variant") {
        let kind = child_path(&path, "kind");
        if offsets.contains_key(&kind) {
            path = kind;
        }
        return issue(
            "unsupported.element",
            &path,
            "Use a stack, row, text, or button element.",
        );
    }
    if let Some(key) = message
        .strip_prefix("unknown field `")
        .and_then(|rest| rest.split('`').next())
    {
        return issue(
            "unsupported.property",
            &child_path(&path, key),
            format!("Remove unsupported property `{key}`."),
        );
    }
    issue(
        "structure.invalid",
        &path,
        message.split(" at line ").next().unwrap_or(&message),
    )
}

// Internally tagged enums buffer their payloads. Check nested schema keys before Serde so
// unsupported elements and properties still point to their exact paths.
fn shape_keys(
    value: &Value,
    path: &str,
    allowed: &[&str],
    required: &[&str],
    issues: &mut Vec<Diagnostic>,
) {
    let Some(object) = value.as_object() else {
        issues.push(issue(
            "structure.expectedObject",
            path,
            "Use a JSON object here.",
        ));
        return;
    };
    for key in object.keys().filter(|key| !allowed.contains(&key.as_str())) {
        issues.push(issue(
            "unsupported.property",
            &child_path(path, key),
            format!("Remove unsupported property `{key}`."),
        ));
    }
    for key in required.iter().filter(|key| !object.contains_key(**key)) {
        issues.push(issue(
            "structure.missingProperty",
            path,
            format!("Add the required property `{key}`."),
        ));
    }
}

fn check_element_shape(
    value: &Value,
    path: &str,
    depth: usize,
    count: &mut usize,
    issues: &mut Vec<Diagnostic>,
) {
    *count += 1;
    if depth > 6 || *count > 64 {
        if !issues.iter().any(|issue| issue.code == "limit.tree") {
            issues.push(issue(
                "limit.tree",
                path,
                "Use up to 64 elements and 6 levels of nesting.",
            ));
        }
        return;
    }
    let kind_path = child_path(path, "kind");
    match value.get("kind").and_then(Value::as_str) {
        Some("stack" | "row") => {
            shape_keys(
                value,
                path,
                &["kind", "children", "style"],
                &["children"],
                issues,
            );
            let children_path = child_path(path, "children");
            if let Some(children) = value.get("children").and_then(Value::as_array) {
                for (index, child) in children.iter().enumerate() {
                    check_element_shape(
                        child,
                        &child_path(&children_path, &index.to_string()),
                        depth + 1,
                        count,
                        issues,
                    );
                }
            } else if value.get("children").is_some() {
                issues.push(issue(
                    "structure.expectedArray",
                    &children_path,
                    "List child elements in a JSON array.",
                ));
            }
        }
        Some("text" | "button") => {
            shape_keys(value, path, &["kind", "text", "style"], &["text"], issues);
            if let Some(text) = value.get("text") {
                let text_path = child_path(path, "text");
                shape_keys(text, &text_path, &["field", "literal"], &[], issues);
                if text
                    .as_object()
                    .is_some_and(|text| text.len() != 1 || !text.values().all(Value::is_string))
                {
                    issues.push(issue(
                        "structure.invalidText",
                        &text_path,
                        "Provide one string property, either field or literal.",
                    ));
                }
            }
        }
        Some(kind) => issues.push(issue(
            "unsupported.element",
            &kind_path,
            format!("Element `{kind}` is unsupported. Use stack, row, text, or button."),
        )),
        None => issues.push(issue(
            "structure.missingKind",
            if value.get("kind").is_some() {
                &kind_path
            } else {
                path
            },
            "Set kind to stack, row, text, or button.",
        )),
    }
    if let Some(style) = value.get("style") {
        let style_path = child_path(path, "style");
        shape_keys(
            style,
            &style_path,
            &[
                "fill", "color", "border", "radius", "padding", "gap", "fontSize", "weight",
            ],
            &[],
            issues,
        );
        if let Some(style) = style.as_object() {
            for (key, value) in style {
                let correct_type = match key.as_str() {
                    "fill" | "color" | "border" => value.is_string(),
                    "radius" | "padding" | "gap" | "fontSize" | "weight" => {
                        value.as_u64().is_some_and(|n| n <= u16::MAX.into())
                    }
                    _ => true,
                };
                if !correct_type {
                    issues.push(issue("value.invalidStyle", &child_path(&style_path, key), "Use a color string or a nonnegative integer as required by this style field."));
                }
            }
        }
    }
}

fn inspect(source: &str, library: &[LibraryEntry]) -> Result<ComponentDefinition, Vec<Diagnostic>> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    // Serde checks strict JSON grammar and recursion limits before source indexing.
    let value: Value = serde_json::from_str(source).map_err(|error| {
        vec![Diagnostic {
            line: Some(error.line().max(1)),
            column: Some(
                source
                    .split('\n')
                    .nth(error.line().saturating_sub(1))
                    .map(|line| {
                        line.char_indices()
                            .filter(|(offset, _)| *offset < error.column().saturating_sub(1))
                            .count()
                            + 1
                    })
                    .unwrap_or(1),
            ),
            ..issue(
                "syntax.invalidJson",
                "",
                "Fix the JSON syntax at this location.",
            )
        }]
    })?;
    let parsed = jsonc_parser::parse_to_ast(source, &Default::default(), &Default::default())
        .ok()
        .and_then(|parsed| parsed.value)
        .ok_or_else(|| {
            vec![issue(
                "syntax.invalidJson",
                "",
                "This file could not be indexed as JSON.",
            )]
        })?;
    let mut offsets = BTreeMap::new();
    let mut duplicates = Vec::new();
    index_source(&parsed, "", &mut offsets, &mut duplicates);
    if !duplicates.is_empty() {
        return Err(duplicates
            .into_iter()
            .map(|(diagnostic, offset)| locate(source, offset, diagnostic))
            .collect());
    }
    if value
        .get("version")
        .and_then(Value::as_u64)
        .is_some_and(|version| version != 1)
    {
        return Err(vec![location(
            source,
            &offsets,
            issue(
                "version.unsupported",
                "/version",
                "Use component format version 1.",
            ),
        )]);
    }
    let mut shape = Vec::new();
    let keys = [
        "version",
        "id",
        "revision",
        "name",
        "defaultWidth",
        "fields",
        "root",
    ];
    shape_keys(&value, "", &keys, &keys, &mut shape);
    if let Some(fields) = value.get("fields").and_then(Value::as_object) {
        for (key, field) in fields {
            shape_keys(
                field,
                &child_path("/fields", key),
                &["label", "default"],
                &["label", "default"],
                &mut shape,
            );
        }
    }
    if let Some(root) = value.get("root") {
        check_element_shape(root, "/root", 1, &mut 0, &mut shape);
    }
    if !shape.is_empty() {
        return Err(shape
            .into_iter()
            .map(|diagnostic| location(source, &offsets, diagnostic))
            .collect());
    }
    let mut deserializer = serde_json::Deserializer::from_str(source);
    let definition: ComponentDefinition = serde_path_to_error::deserialize(&mut deserializer)
        .map_err(|error| vec![location(source, &offsets, shape_issue(error, &offsets))])?;
    let mut diagnostics = definition.diagnostics();
    if let Some(conflict) = conflict(library, &definition) {
        diagnostics.push(conflict);
    }
    if diagnostics.is_empty() {
        Ok(definition)
    } else {
        Err(diagnostics
            .into_iter()
            .map(|diagnostic| location(source, &offsets, diagnostic))
            .collect())
    }
}

#[derive(Deserialize, Serialize)]
struct LibraryEntry {
    definition: ComponentDefinition,
    visible: bool,
}

#[derive(Serialize)]
pub(crate) struct ComponentReview {
    name: String,
    diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    definition: Option<ComponentDefinition>,
}

fn review(name: String, result: Result<ComponentDefinition, Vec<Diagnostic>>) -> ComponentReview {
    match result {
        Ok(definition) => ComponentReview {
            name,
            definition: Some(definition),
            diagnostics: Vec::new(),
        },
        Err(diagnostics) => ComponentReview {
            name,
            definition: None,
            diagnostics,
        },
    }
}

// Remember revisions even after removing them from the palette.
fn conflict(library: &[LibraryEntry], definition: &ComponentDefinition) -> Option<Diagnostic> {
    library.iter().find(|entry| same_revision(&entry.definition, &definition.id, definition.revision)
        && entry.definition != *definition)
        .map(|_| issue("conflict.revision", "/revision", "Different content already uses this revision. Increment the revision and import again."))
}

fn same_revision(definition: &ComponentDefinition, id: &str, revision: u32) -> bool {
    definition.id == id && definition.revision == revision
}

fn library_path(app: &AppHandle) -> Result<PathBuf, super::Error> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("prototype-components.json"))
        .map_err(|_| super::Error::StorageUnavailable)
}

fn read_library(path: &std::path::Path) -> Result<Vec<LibraryEntry>, super::Error> {
    match fs::File::open(path) {
        Ok(file) => serde_json::from_reader::<_, Vec<LibraryEntry>>(file)
            .ok()
            .filter(|entries| entries.iter().all(|entry| entry.definition.valid()))
            .ok_or(super::Error::StorageUnavailable),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(_) => Err(super::Error::StorageUnavailable),
    }
}

fn write_library(path: &std::path::Path, library: &[LibraryEntry]) -> Result<(), super::Error> {
    let unavailable = |_| super::Error::StorageUnavailable;
    let parent = path.parent().ok_or(super::Error::StorageUnavailable)?;
    fs::create_dir_all(parent).map_err(unavailable)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(unavailable)?;
    serde_json::to_writer(&mut file, library).map_err(|_| super::Error::StorageUnavailable)?;
    file.flush().map_err(unavailable)?;
    file.as_file().sync_all().map_err(unavailable)?;
    file.persist(path)
        .map_err(|_| super::Error::StorageUnavailable)?;
    Ok(())
}

// The file picker grants one local read. Review does not modify either store.
#[tauri::command]
pub(crate) async fn prototype_component_choose(
    app: AppHandle,
) -> Result<Option<ComponentReview>, super::Error> {
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("WITS component JSON", &["json"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|_| super::Error::StorageUnavailable)?;
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut bytes = Vec::new();
    let result = if fs::File::open(path)
        .and_then(|file| file.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .is_err()
    {
        Err(vec![issue(
            "io.read",
            "",
            "Choose a readable JSON file and try again.",
        )])
    } else if bytes.len() as u64 > MAX_BYTES {
        Err(vec![issue(
            "limit.file",
            "",
            "Use a component file no larger than 64 KiB.",
        )])
    } else if let Ok(source) = std::str::from_utf8(&bytes) {
        inspect(source, &read_library(&library_path(&app)?)?)
    } else {
        Err(vec![issue(
            "syntax.encoding",
            "",
            "Save the component file as UTF-8 and try again.",
        )])
    };
    Ok(Some(review(name, result)))
}

// Validate again under the write lock so concurrent approvals cannot replace a revision.
#[tauri::command]
pub(crate) fn prototype_component_install(
    app: AppHandle,
    definition: ComponentDefinition,
) -> Result<ComponentReview, super::Error> {
    let _lock = LIBRARY_LOCK
        .lock()
        .map_err(|_| super::Error::StorageUnavailable)?;
    install_at(&library_path(&app)?, definition)
}

fn install_at(
    path: &std::path::Path,
    definition: ComponentDefinition,
) -> Result<ComponentReview, super::Error> {
    let mut library = read_library(path)?;
    let mut diagnostics = definition.diagnostics();
    if serde_json::to_vec(&definition).map_or(true, |bytes| bytes.len() as u64 > MAX_BYTES) {
        diagnostics.push(issue(
            "limit.file",
            "",
            "Use a component file no larger than 64 KiB.",
        ));
    }
    if let Some(conflict) = conflict(&library, &definition) {
        diagnostics.push(conflict);
    }
    let name = definition.name.clone();
    if !diagnostics.is_empty() {
        return Ok(review(name, Err(diagnostics)));
    }
    if let Some(existing) = library
        .iter_mut()
        .find(|entry| same_revision(&entry.definition, &definition.id, definition.revision))
    {
        if existing.visible {
            return Ok(review(name, Ok(definition)));
        }
        existing.visible = true;
    } else {
        library.push(LibraryEntry {
            definition: definition.clone(),
            visible: true,
        });
    }
    write_library(path, &library)?;
    Ok(review(name, Ok(definition)))
}

#[tauri::command]
pub(crate) fn prototype_component_list(
    app: AppHandle,
) -> Result<Vec<ComponentDefinition>, super::Error> {
    let mut definitions: Vec<_> = read_library(&library_path(&app)?)?
        .into_iter()
        .filter(|entry| entry.visible)
        .map(|entry| entry.definition)
        .collect();
    definitions.sort_by(|left, right| {
        (&left.name, &left.id, left.revision).cmp(&(&right.name, &right.id, right.revision))
    });
    Ok(definitions)
}

#[tauri::command]
pub(crate) fn prototype_component_remove(
    app: AppHandle,
    id: String,
    revision: u32,
) -> Result<(), super::Error> {
    let _lock = LIBRARY_LOCK
        .lock()
        .map_err(|_| super::Error::StorageUnavailable)?;
    remove_at(&library_path(&app)?, &id, revision)
}

fn remove_at(path: &std::path::Path, id: &str, revision: u32) -> Result<(), super::Error> {
    let mut library = read_library(path)?;
    if let Some(entry) = library
        .iter_mut()
        .find(|entry| entry.visible && same_revision(&entry.definition, id, revision))
    {
        entry.visible = false;
        write_library(path, &library)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRICING: &str =
        include_str!("../../../examples/prototype-builder/pricing-card.wits-component.json");

    // Review is read-only, identical installations are idempotent, and hidden revisions
    // still reserve their identity after reopening the library from disk.
    #[test]
    fn installs_and_hides_revisions_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.json");
        let definition = inspect(PRICING, &read_library(&path).unwrap())
            .ok()
            .unwrap();
        assert!(!path.exists());
        assert!(
            install_at(&path, definition.clone())
                .unwrap()
                .diagnostics
                .is_empty()
        );
        let bytes = fs::read(&path).unwrap();
        assert!(
            install_at(&path, definition.clone())
                .unwrap()
                .diagnostics
                .is_empty()
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        remove_at(&path, &definition.id, definition.revision).unwrap();
        let library = read_library(&path).unwrap();
        assert!(!library[0].visible);
        let mut changed = definition.clone();
        changed.name = "Different content".to_owned();
        let result = install_at(&path, changed.clone()).unwrap();
        assert_eq!(result.diagnostics[0].code, "conflict.revision");
        changed.revision += 1;
        assert!(install_at(&path, changed).unwrap().diagnostics.is_empty());
        let reopened = read_library(&path).unwrap();
        assert!(!reopened[0].visible);
        assert!(reopened[1].visible);
        assert!(reopened[0].definition == definition);
    }

    // Report the exact nested location even when Windows line endings and Unicode are present.
    #[test]
    fn indexes_unsupported_elements_references_and_styles() {
        let source = PRICING
            .replace("\"Pricing card\"", "\"定价卡\"")
            .replace("\"kind\": \"button\"", "\"kind\": \"chart\"")
            .replace('\n', "\r\n");
        let diagnostic = inspect(&source, &[]).err().unwrap().remove(0);
        assert_eq!(diagnostic.code, "unsupported.element");
        assert_eq!(diagnostic.path, "/root/children/2/kind");
        let line = source.lines().nth(diagnostic.line.unwrap() - 1).unwrap();
        assert_eq!(
            line.chars()
                .skip(diagnostic.column.unwrap() - 1)
                .take(7)
                .collect::<String>(),
            "\"chart\""
        );

        let source = PRICING
            .replace("\"field\": \"action\"", "\"field\": \"missing\"")
            .replace("\"radius\": 24", "\"radius\": 550");
        let diagnostics = inspect(&source, &[]).err().unwrap();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "reference.unknownField"
                    && diagnostic.path.ends_with("/text/field"))
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "value.invalidStyle"
                    && diagnostic.path.ends_with("/style/radius"))
        );
    }

    #[test]
    fn rejects_duplicate_keys_syntax_and_conflicting_revisions() {
        let duplicate = "{\"name\":\"定价\",\"name\":\"second\"}";
        let diagnostic = inspect(duplicate, &[]).err().unwrap().remove(0);
        assert_eq!(diagnostic.code, "structure.duplicateKey");
        assert_eq!(diagnostic.line, Some(1));
        assert_eq!(diagnostic.column, Some(14));
        let syntax = inspect("{\"a\":}", &[]).err().unwrap().remove(0);
        assert_eq!(syntax.code, "syntax.invalidJson");
        assert_eq!(syntax.line, Some(1));
        let definition = inspect(PRICING, &[]).ok().unwrap();
        let library = [LibraryEntry {
            definition,
            visible: false,
        }];
        assert!(inspect(PRICING, &library).is_ok());
        let changed = PRICING.replace("Choose Studio", "Buy Studio");
        let conflict = inspect(&changed, &library).err().unwrap().remove(0);
        assert_eq!(conflict.code, "conflict.revision");
        assert_eq!(conflict.line, Some(4));
        assert!(
            inspect(
                &changed.replace("\"revision\": 1", "\"revision\": 2"),
                &library
            )
            .is_ok()
        );
    }
}
