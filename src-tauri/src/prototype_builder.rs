//! Imports GitHub starter data and stores independent local prototype snapshots.

use std::{fs, io::Write, path::PathBuf, time::Duration};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use url::Url;
use uuid::Uuid;

const STARTER_PATH: &str = "examples/prototype-builder/wits-starter.json";
const MAX_STARTER_BYTES: usize = 64 * 1024;

// Own the connection pool for the lifetime of the desktop app.
pub(crate) struct GitHubClient(Result<Client, reqwest::Error>);

impl GitHubClient {
    pub(crate) fn new() -> Self {
        Self(
            Client::builder()
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("wits-tools-prototype-builder")
                .build(),
        )
    }
}

// Keep the imported data contract separate from local snapshot metadata.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Starter {
    version: u8,
    title: String,
    cards: Vec<Card>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Card {
    id: String,
    title: String,
    body: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Imported {
    source_url: String,
    starter: Starter,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Variant {
    id: Option<Uuid>,
    name: String,
    source_url: String,
    starter: Starter,
}

// Return stable error codes for action-specific messages in Vue.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Error {
    InvalidUrl,
    MissingStarter,
    InvalidStarter,
    ImportUnavailable,
    InvalidName,
    StorageUnavailable,
}

// Build the API URL only from validated GitHub owner and repository segments.
fn repository(source: &str) -> Result<(String, String), Error> {
    let url = Url::parse(source.trim()).map_err(|_| Error::InvalidUrl)?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::InvalidUrl);
    }
    let path = url.path().trim_end_matches('/');
    let Some((owner, repo)) = path.trim_start_matches('/').split_once('/') else {
        return Err(Error::InvalidUrl);
    };
    let valid = |part: &str| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    };
    if !valid(owner) || !valid(repo) {
        return Err(Error::InvalidUrl);
    }
    Ok((
        format!("https://github.com/{owner}/{repo}"),
        format!("https://api.github.com/repos/{owner}/{repo}/contents/{STARTER_PATH}"),
    ))
}

// Stable, unique IDs preserve component identity as cards move.
fn validate(starter: &Starter) -> Result<(), Error> {
    let mut ids = std::collections::HashSet::new();
    if starter.version != 1
        || starter.title.trim().is_empty()
        || starter.cards.is_empty()
        || starter.cards.iter().any(|card| {
            card.id.trim().is_empty() || card.title.trim().is_empty() || !ids.insert(&card.id)
        })
    {
        return Err(Error::InvalidStarter);
    }
    Ok(())
}

// Limit the response as it streams, including when there is no Content-Length header.
async fn fetch(client: &Client, source: &str) -> Result<Imported, Error> {
    let (source_url, endpoint) = repository(source)?;
    let mut response = client
        .get(endpoint)
        .header("Accept", "application/vnd.github.raw+json")
        .send()
        .await
        .map_err(|_| Error::ImportUnavailable)?;
    match response.status() {
        reqwest::StatusCode::OK => {}
        reqwest::StatusCode::NOT_FOUND => return Err(Error::MissingStarter),
        _ => return Err(Error::ImportUnavailable),
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| Error::ImportUnavailable)?
    {
        if bytes.len() + chunk.len() > MAX_STARTER_BYTES {
            return Err(Error::InvalidStarter);
        }
        bytes.extend_from_slice(&chunk);
    }
    let starter = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidStarter)?;
    validate(&starter)?;
    Ok(Imported {
        source_url,
        starter,
    })
}

#[tauri::command]
pub(crate) async fn prototype_import(
    client: State<'_, GitHubClient>,
    source: String,
) -> Result<Imported, Error> {
    fetch(
        client.0.as_ref().map_err(|_| Error::ImportUnavailable)?,
        &source,
    )
    .await
}

// Resolve all snapshot locations inside application-owned storage.
fn storage(app: &AppHandle) -> Result<PathBuf, Error> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("prototype-builder"))
        .map_err(|_| Error::StorageUnavailable)
}

#[tauri::command]
pub(crate) async fn prototype_save(app: AppHandle, variant: Variant) -> Result<Uuid, Error> {
    save(storage(&app)?, variant)
}

// Flush the staged snapshot before replacing an existing variant.
fn save(dir: PathBuf, mut variant: Variant) -> Result<Uuid, Error> {
    validate(&variant.starter)?;
    if variant.name.trim().is_empty() {
        return Err(Error::InvalidName);
    }
    variant.name = variant.name.trim().to_owned();
    variant.source_url = repository(&variant.source_url)?.0;
    let id = *variant.id.get_or_insert_with(Uuid::new_v4);
    fs::create_dir_all(&dir).map_err(|_| Error::StorageUnavailable)?;
    let mut file = tempfile::NamedTempFile::new_in(&dir).map_err(|_| Error::StorageUnavailable)?;
    serde_json::to_writer(&mut file, &variant).map_err(|_| Error::StorageUnavailable)?;
    file.flush().map_err(|_| Error::StorageUnavailable)?;
    file.as_file()
        .sync_all()
        .map_err(|_| Error::StorageUnavailable)?;
    file.persist(dir.join(format!("{id}.json")))
        .map_err(|_| Error::StorageUnavailable)?;
    Ok(id)
}

#[tauri::command]
pub(crate) async fn prototype_list(app: AppHandle) -> Result<Vec<Variant>, Error> {
    list(storage(&app)?)
}

// Ignore staging files and return saved snapshots in display order.
fn list(dir: PathBuf) -> Result<Vec<Variant>, Error> {
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
        let variant: Variant =
            serde_json::from_reader(file).map_err(|_| Error::StorageUnavailable)?;
        validate(&variant.starter)?;
        variants.push(variant);
    }
    variants.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    Ok(variants)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verify the committed sample and reject IDs that could confuse the editor.
    #[test]
    fn validates_sample_and_rejects_duplicate_ids() {
        let mut starter: Starter = serde_json::from_str(include_str!(
            "../../examples/prototype-builder/wits-starter.json"
        ))
        .unwrap();
        validate(&starter).unwrap();
        starter.cards[1].id = starter.cards[0].id.clone();
        assert!(matches!(validate(&starter), Err(Error::InvalidStarter)));
    }

    // Restrict fetching to repository URLs and a fixed GitHub API endpoint.
    #[test]
    fn builds_endpoint_from_repository_url() {
        let (_, endpoint) = repository("https://github.com/owner/repo/").unwrap();
        assert_eq!(
            endpoint,
            format!("https://api.github.com/repos/owner/repo/contents/{STARTER_PATH}")
        );
        assert!(repository("https://github.com.evil.test/owner/repo").is_err());
        assert!(repository("https://github.com/owner/repo/tree/main").is_err());
    }

    // Saving a copy and overwriting one snapshot leave the other snapshot independent.
    #[test]
    fn saves_and_reopens_independent_variants() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("variants");
        let make_variant = |id, name: &str| Variant {
            id,
            name: name.to_owned(),
            source_url: "https://github.com/owner/repo".to_owned(),
            starter: serde_json::from_str(include_str!(
                "../../examples/prototype-builder/wits-starter.json"
            ))
            .unwrap(),
        };
        let first = save(dir.clone(), make_variant(None, "First")).unwrap();
        let second = save(dir.clone(), make_variant(None, "Second")).unwrap();
        let mut revised = make_variant(Some(first), "First");
        revised.starter.cards[0].title = "Revised title".to_owned();
        assert_eq!(save(dir.clone(), revised).unwrap(), first);
        let variants = list(dir).unwrap();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].id, Some(first));
        assert_eq!(variants[0].starter.cards[0].title, "Revised title");
        assert_eq!(variants[1].id, Some(second));
        assert_eq!(variants[1].starter.cards[0].title, "Release status");
    }
}
