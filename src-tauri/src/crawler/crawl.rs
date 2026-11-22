//! Runs static, browser, and adaptive crawl jobs through Crawlee's request queue.

use std::{
    collections::HashSet,
    fs::File,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crawlee::basic::{BasicCrawlerBuilder, Middleware, ProxySource};
use crawlee::{
    BasicContext, BasicCrawler, CrawlingContext, HtmlContext, HtmlCrawler, ProxyConfiguration,
    Request, Services,
};
use futures_util::{StreamExt, stream};
use regex::RegexSet;
use serde::{Deserialize, Serialize};
use tokio::{
    sync::{Mutex, Semaphore},
    time,
};
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

use super::{
    Error, Result,
    browser::{Browser, Resource},
    http_url, network,
    storage::{Asset, Store},
};

/// Keep the legacy mode values in JSON exports while using engine-neutral names in the UI.
#[derive(Clone, Copy, Default, Deserialize, PartialEq, Serialize)]
pub enum Mode {
    #[default]
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "always")]
    Browser,
    #[serde(rename = "never")]
    Static,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub start_urls: Vec<String>,
    pub max_pages: u32,
    pub max_depth: u32,
    #[serde(rename = "usePlaywright", alias = "mode")]
    pub mode: Mode,
    pub concurrency: usize,
    pub request_delay_millis: u64,
    pub download_assets: bool,
    // Older jobs and callers keep using managed storage when no folder is supplied.
    pub asset_folder: Option<PathBuf>,
    pub allowed_domains: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub respect_robots_txt: bool,
    pub proxy_url: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            start_urls: Vec::new(),
            max_pages: 100,
            max_depth: 5,
            mode: Mode::Auto,
            concurrency: 2,
            request_delay_millis: 500,
            download_assets: true,
            asset_folder: None,
            allowed_domains: Vec::new(),
            exclude_patterns: Vec::new(),
            respect_robots_txt: true,
            proxy_url: None,
        }
    }
}

impl Config {
    /// Validate the crawl budget, seed URLs, proxy, and exclusion rules before queuing work.
    pub fn validate(&self) -> Result<()> {
        if self.start_urls.is_empty()
            || (self.download_assets
                && self
                    .asset_folder
                    .as_ref()
                    .is_some_and(|folder| !folder.is_absolute()))
            || self.max_pages == 0
            || !(1..=20).contains(&self.concurrency)
            || self.allowed_domains.iter().any(|domain| {
                Url::parse(&format!("http://{domain}/"))
                    .ok()
                    .and_then(|url| url.host_str().map(str::to_owned))
                    .is_none_or(|host| !host.eq_ignore_ascii_case(domain))
            })
        {
            return Err(Error::InvalidConfig);
        }
        for seed in &self.start_urls {
            http_url(seed)?;
        }
        network::proxy(self.proxy_url.as_deref())?;
        RegexSet::new(&self.exclude_patterns).map_err(|_| Error::InvalidConfig)?;
        Ok(())
    }

    /// Persist the proxy endpoint without its credentials.
    pub(super) fn redacted(&self) -> Self {
        let mut config = self.clone();
        config.proxy_url = network::proxy(self.proxy_url.as_deref())
            .ok()
            .flatten()
            // A DoH path can contain a private provider profile identifier.
            .filter(|proxy| proxy.scheme() != "doh")
            .map(|mut proxy| {
                let _ = proxy.set_password(None);
                let _ = proxy.set_username("");
                proxy.to_string()
            });
        config
    }
}

pub struct CollectedPage {
    pub url: Url,
    pub html: String,
    pub title: String,
    pub text: String,
    pub headers_json: Option<String>,
    pub depth: u32,
    pub status: u16,
}

struct Runner {
    id: Uuid,
    store: Store,
    config: Config,
    seeds: Vec<Url>,
    exclude: RegexSet,
    proxy: Option<Url>,
    client: reqwest::Client,
    browser_path: PathBuf,
    browser: Mutex<Option<Browser>>,
    stop: CancellationToken,
    asset_slots: Semaphore,
    assets: Mutex<HashSet<Url>>,
    failed: AtomicBool,
}

/// Hold the persisted job's lease until its final status and browser cleanup are complete.
pub async fn run(
    store: Store,
    id: Uuid,
    config: Config,
    browser_path: PathBuf,
    lease: File,
    stop: CancellationToken,
) -> Result<()> {
    let result = execute(store.clone(), id, config, browser_path, stop.clone()).await;
    let status = match &result {
        Err(Error::Cancelled) => "stopped",
        Err(_) => "failed",
        Ok(()) if stop.is_cancelled() => "stopped",
        Ok(()) => "completed",
    };
    store.finish(id, status)?;
    drop(lease);
    result
}

async fn execute(
    store: Store,
    id: Uuid,
    config: Config,
    browser_path: PathBuf,
    stop: CancellationToken,
) -> Result<()> {
    config.validate()?;
    let proxy = network::proxy(config.proxy_url.as_deref())?;
    let seeds = config
        .start_urls
        .iter()
        .map(|url| http_url(url))
        .collect::<Result<Vec<_>>>()?;
    let exclude = RegexSet::new(&config.exclude_patterns).map_err(|_| Error::InvalidConfig)?;
    let client = network::client(proxy.as_ref(), true)?;
    let runner = Arc::new(Runner {
        id,
        store,
        config,
        seeds,
        exclude,
        proxy,
        client,
        browser_path,
        browser: Mutex::new(None),
        stop: stop.clone(),
        asset_slots: Semaphore::new(8),
        assets: Mutex::new(HashSet::new()),
        failed: AtomicBool::new(false),
    });
    let seeds = runner
        .seeds
        .iter()
        .filter(|url| runner.allows(url))
        .map(Url::as_str)
        .collect::<Vec<_>>();
    if seeds.is_empty() {
        return Err(Error::InvalidConfig);
    }
    // Use a single request budget, including URLs discovered on browser-rendered pages.
    let result = match runner.config.mode {
        Mode::Browser => {
            let browser = Arc::clone(&runner);
            let builder = BasicCrawler::builder().request_handler(move |ctx: BasicContext| {
                let browser = Arc::clone(&browser);
                async move { browser.render(&ctx).await.map_err(Into::into) }
            });
            run_queue(builder, &runner, seeds, stop).await
        }
        _ => {
            let html = Arc::clone(&runner);
            let builder = HtmlCrawler::builder().request_handler(move |ctx: HtmlContext| {
                let html = Arc::clone(&html);
                async move { html.html(&ctx).await }
            });
            run_queue(builder, &runner, seeds, stop).await
        }
    };
    if let Some(browser) = runner.browser.lock().await.take() {
        browser.shutdown().await;
    }
    result?;
    if runner.failed.load(Ordering::Relaxed) && runner.store.job(id)?.pages_crawled == 0 {
        return Err(Error::Network);
    }
    Ok(())
}

/// Configure the static and browser pipelines with the same queue, policies, and cancellation.
async fn run_queue<P: Middleware<BasicContext>>(
    builder: BasicCrawlerBuilder<P>,
    runner: &Arc<Runner>,
    seeds: Vec<&str>,
    stop: CancellationToken,
) -> Result<()>
where
    P::Out: CrawlingContext,
{
    // Crawlee accepts forward proxies only. Its own requests need the same DNS route.
    let dns_relay = if let Some(endpoint) = &runner.proxy {
        super::proxy::DnsRelay::start(endpoint).await?
    } else {
        None
    };
    let proxy = dns_relay
        .as_ref()
        .map(|relay| &relay.url)
        .or(runner.proxy.as_ref())
        .map(|url| {
            ProxyConfiguration::new([url.as_str()])
                .map(|config| Arc::new(config) as Arc<dyn ProxySource>)
                .map_err(|_| Error::InvalidProxy)
        })
        .transpose()?;
    let delay = Duration::from_millis(runner.config.request_delay_millis);
    let failed = Arc::clone(&runner);
    let mut builder = builder
        .services(Services::in_memory())
        .respect_robots_txt(runner.config.respect_robots_txt)
        .same_domain_delay(delay)
        .max_requests_per_crawl(u64::from(runner.config.max_pages))
        .max_concurrency(runner.config.concurrency)
        .max_request_retries(3)
        .request_handler_timeout(Duration::from_secs(120))
        .failed_request_handler(move |ctx: BasicContext, _| {
            let failed = Arc::clone(&failed);
            async move {
                if failed.config.mode == Mode::Auto && failed.render(&ctx).await.is_ok() {
                    return Ok(());
                }
                failed.failed.store(true, Ordering::Relaxed);
                Ok(())
            }
        });
    if let Some(proxy) = proxy {
        builder = builder.proxy_configuration(proxy);
    }
    let crawler = builder.build().map_err(|_| Error::InvalidConfig)?;
    let crawl = crawler.run(seeds);
    tokio::pin!(crawl);
    // Observe desktop cancellation immediately and cross-process cancellation on a short interval.
    let watch = async {
        let mut poll = time::interval(Duration::from_millis(350));
        loop {
            tokio::select! {
                _ = stop.cancelled() => break,
                _ = poll.tick() => if runner.store.stopping(runner.id).unwrap_or(false) { break; },
            }
        }
    };
    tokio::select! {
        result = &mut crawl => result.map(|_| ()).map_err(|_| Error::Network),
        _ = watch => {
            stop.cancel();
            crawler.stop(Some("Stopped by user"));
            let _ = time::timeout(Duration::from_secs(2), &mut crawl).await;
            Err(Error::Cancelled)
        }
    }
}

impl Runner {
    /// Restrict traversal to the explicit domain list or the original URL subtree.
    fn allows(&self, url: &Url) -> bool {
        if self.exclude.is_match(url.as_str()) {
            return false;
        }
        if !self.config.allowed_domains.is_empty() {
            return url.host_str().is_some_and(|host| {
                self.config
                    .allowed_domains
                    .iter()
                    .any(|domain| host.eq_ignore_ascii_case(domain))
            });
        }
        self.seeds.iter().any(|seed| {
            seed.origin() == url.origin()
                && (seed.path() == url.path()
                    || url
                        .path()
                        .starts_with(&format!("{}/", seed.path().trim_end_matches('/'))))
        })
    }

    /// Extract titles, body text, linked pages, and asset references with the Crawlee HTML parser.
    async fn html(&self, ctx: &HtmlContext) -> anyhow::Result<()> {
        let content_type = ctx
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok());
        if content_type.is_some_and(|value| {
            !value.contains("text/html") && !value.contains("application/xhtml+xml")
        }) {
            return Ok(());
        }
        let url = ctx.url().clone();
        if !self.allows(&url) {
            return Ok(());
        }
        let (title, text, links, assets) = ctx.with_html(|doc| {
            // Script source does not count toward the visible text threshold for Auto mode.
            let text = doc.select_first("body").ok().flatten().map(|body| {
                body.inner().descendants().filter_map(|node| {
                    let text = node.value().as_text()?;
                    let hidden = node.ancestors().filter_map(|node| node.value().as_element())
                        .any(|element| matches!(element.name(), "script" | "style" | "template" | "noscript"));
                    (!hidden).then_some(text.as_ref())
                }).collect::<Vec<&str>>().join(" ")
            }).unwrap_or_default();
            let links = doc.select("a[href]").ok().into_iter().flatten()
                .filter_map(|anchor| anchor.attr("href").map(str::to_owned)).collect::<Vec<_>>();
            let mut assets = Vec::new();
            for (selector, attribute) in [("img[src],script[src],video[src],audio[src],source[src],track[src],embed[src]", "src"),
                ("link[rel=stylesheet],link[rel=icon],link[rel=preload]", "href"), ("video[poster]", "poster")] {
                assets.extend(doc.select(selector).ok().into_iter().flatten()
                    .filter_map(|element| element.attr(attribute).map(str::to_owned)));
            }
            (doc.title().unwrap_or_default(), text.split_whitespace().collect::<Vec<_>>().join(" "), links, assets)
        }).await.map_err(|_| Error::Network)?;
        if self.config.mode == Mode::Auto && text.chars().count() < 100 {
            return self.render(ctx).await.map_err(Into::into);
        }
        let headers = ctx
            .headers()
            .iter()
            .filter(|(name, _)| {
                matches!(
                    name.as_str(),
                    "content-type" | "content-length" | "last-modified" | "etag"
                )
            })
            .filter_map(|(name, value)| Some((name.as_str(), value.to_str().ok()?)))
            .collect::<Vec<_>>();
        let page = CollectedPage {
            url,
            html: ctx.text().into(),
            title,
            text,
            status: ctx.status().as_u16(),
            depth: ctx.request().crawl_depth(),
            headers_json: serde_json::to_string(&headers).ok(),
        };
        let resources = assets
            .into_iter()
            .map(|url| Resource {
                url,
                ..Resource::default()
            })
            .collect();
        self.record(ctx, page, links, resources)
            .await
            .map_err(Into::into)
    }

    /// Launch a browser only when required and reuse it within the job's proxy session.
    async fn render(&self, ctx: &BasicContext) -> Result<()> {
        let url = http_url(&ctx.request().url)?;
        if !self.allows(&url) {
            return Ok(());
        }
        let mut slot = self.browser.lock().await;
        if slot.is_none() {
            let local = self.seeds.iter().any(local_seed);
            *slot = Some(
                Browser::start(
                    &self.browser_path,
                    self.proxy.as_ref(),
                    self.config.respect_robots_txt,
                    local,
                    super::diagnostics::Trace::new(false),
                )
                .await?,
            );
        }
        let browser = slot.as_mut().ok_or(Error::BrowserFailed)?;
        let rendered = tokio::select! {
            _ = self.stop.cancelled() => Err(Error::Cancelled),
            result = browser.page(&url) => result,
        };
        if rendered.is_err() {
            *slot = None;
        }
        drop(slot);
        let rendered = rendered?;
        let final_url = http_url(&rendered.url)?;
        if !self.allows(&final_url) {
            return Ok(());
        }
        let resources = rendered
            .resources
            .into_iter()
            .filter(|entry| entry.url != rendered.url && !entry.mime.starts_with("text/html"))
            .collect();
        let page = CollectedPage {
            url: final_url,
            html: rendered.html,
            title: rendered.title,
            text: rendered.text,
            status: rendered.status,
            depth: ctx.request().crawl_depth(),
            headers_json: None,
        };
        self.record(ctx, page, rendered.links, resources).await
    }

    /// Save a page before queueing its links, and keep asset transfers bounded across handlers.
    async fn record(
        &self,
        ctx: &BasicContext,
        page: CollectedPage,
        links: Vec<String>,
        assets: Vec<Resource>,
    ) -> Result<()> {
        self.store.page(self.id, &page)?;
        if page.depth < self.config.max_depth {
            let next = links
                .into_iter()
                .filter_map(|link| page.url.join(&link).ok())
                .filter_map(|url| http_url(url.as_str()).ok())
                .filter(|url| self.allows(url))
                .filter_map(|url| {
                    Request::builder(url.as_str())
                        .crawl_depth(page.depth + 1)
                        .build()
                        .ok()
                })
                .collect::<Vec<_>>();
            ctx.add_requests(next).await.map_err(|_| Error::Network)?;
        }
        if self.config.download_assets {
            stream::iter(assets)
                .for_each_concurrent(4, |asset| self.download_asset(&page.url, asset))
                .await;
        }
        Ok(())
    }

    async fn download_asset(&self, page: &Url, asset: Resource) {
        let Some(url) = page
            .join(&asset.url)
            .ok()
            .and_then(|url| http_url(url.as_str()).ok())
        else {
            return;
        };
        if !self.assets.lock().await.insert(url.clone()) {
            return;
        }
        let saved = tokio::select! {
            _ = self.stop.cancelled() => Err(Error::Cancelled),
            result = self.save_asset(page, &url, asset) => result,
        };
        if saved.is_err() {
            self.assets.lock().await.remove(&url);
        }
    }

    async fn save_asset(&self, page: &Url, url: &Url, asset: Resource) -> Result<()> {
        let _slot = self
            .asset_slots
            .acquire()
            .await
            .map_err(|_| Error::Cancelled)?;
        let mut request = self
            .client
            .get(url.clone())
            .headers(network::replay_headers(&asset.headers, true));
        if page.origin() == url.origin() {
            request = request.header("referer", page.as_str());
        }
        let response = request.send().await.map_err(|_| Error::Network)?;
        let mime = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/octet-stream")
            .split(';')
            .next()
            .unwrap_or_default()
            .to_owned();
        // Use an isolated job folder at the chosen destination to avoid mixing crawl files.
        let directory = self.config.asset_folder.as_ref().map_or_else(
            || self.store.asset_dir(self.id),
            |folder| folder.join(self.id.to_string()),
        );
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(|_| Error::File)?;
        let temporary = tempfile::NamedTempFile::new_in(&directory)
            .map_err(|_| Error::File)?
            .into_temp_path();
        let length = network::save(response, temporary.as_ref(), |_| {}).await?;
        let name = url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .unwrap_or("asset");
        let name = format!(
            "{}-{}",
            Uuid::new_v4().simple(),
            super::media::filename(name)
        );
        let path = directory.join(&name);
        temporary
            .persist_noclobber(&path)
            .map_err(|_| Error::File)?;
        let recorded = self.store.asset(
            self.id,
            &Asset {
                source_url: url.as_str().into(),
                // Preserve legacy relative paths for managed assets and report the full custom path.
                local_path: self.config.asset_folder.as_ref().map_or_else(
                    || format!("{}/{name}", self.id),
                    |_| path.to_string_lossy().into_owned(),
                ),
                mime_type: mime,
                size_bytes: length.try_into().map_err(|_| Error::File)?,
            },
        );
        if recorded.is_err() {
            let _ = tokio::fs::remove_file(path).await;
        }
        recorded
    }
}

/// Permit explicitly selected loopback and LAN seed fixtures in the browser session.
pub(super) fn local_seed(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_private(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback() || ip.is_unique_local(),
        _ => false,
    }
}
