//! Resolves crawler requests through a user-selected DNS-over-HTTPS endpoint.

use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use hickory_proto::{
    op::{Message, MessageType, Query, ResponseCode},
    rr::{Name, RData, RecordType},
};
use reqwest::dns::{Addrs, Name as HttpName, Resolve, Resolving};
use url::Url;

use super::{Error, Result, network};

/// Normalize recognized public resolver addresses and allow any explicit DoH provider URL.
pub(super) fn endpoint(value: &str) -> Option<Result<Url>> {
    let value = match value {
        "1.1.1.1" | "1.0.0.1" => "https://cloudflare-dns.com/dns-query",
        "1.1.1.2" | "1.0.0.2" => "https://security.cloudflare-dns.com/dns-query",
        "1.1.1.3" | "1.0.0.3" => "https://family.cloudflare-dns.com/dns-query",
        "8.8.8.8" | "8.8.4.4" => "https://dns.google/dns-query",
        "9.9.9.9" | "149.112.112.112" => "https://dns.quad9.net/dns-query",
        "94.140.14.14" | "94.140.15.15" => "https://dns.adguard-dns.com/dns-query",
        _ => value,
    };
    let explicit = value.strip_prefix("doh://");
    let parsed =
        Url::parse(&explicit.map_or_else(|| value.to_owned(), |host| format!("https://{host}")));
    if explicit.is_none()
        && !parsed.as_ref().is_ok_and(|url| {
            url.scheme() == "https"
                && (url.path() == "/dns-query"
                    || url.host_str() == Some("dns.nextdns.io") && url.path() != "/")
        })
    {
        return None;
    }
    Some(
        parsed
            .ok()
            .filter(|url| {
                url.host().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none()
            })
            .and_then(|url| Url::parse(&url.as_str().replacen("https://", "doh://", 1)).ok())
            .ok_or(Error::InvalidProxy),
    )
}

#[derive(Clone)]
pub(super) struct Doh {
    endpoint: Url,
    client: reqwest::Client,
}

impl Doh {
    /// Bootstrap only the resolver through the system and never redirect DNS queries elsewhere.
    pub fn new(endpoint: Url) -> Result<Arc<Self>> {
        let endpoint = Url::parse(&endpoint.as_str().replacen("doh://", "https://", 1))
            .map_err(|_| Error::InvalidProxy)?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(6))
            .build()
            .map_err(|_| Error::Network)?;
        Ok(Arc::new(Self { endpoint, client }))
    }

    /// Query both address families so public DNS can serve IPv4 and IPv6 destinations.
    pub async fn addresses(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>> {
        let mut name = Name::from_ascii(host).map_err(|_| Error::Network)?;
        name.set_fqdn(true);
        let (v4, v6) = tokio::join!(
            self.query(&name, RecordType::A),
            self.query(&name, RecordType::AAAA)
        );
        let addresses = v4
            .unwrap_or_default()
            .into_iter()
            .chain(v6.unwrap_or_default())
            .filter(|ip| public_address(*ip))
            .map(|ip| SocketAddr::new(ip, port))
            .collect::<Vec<_>>();
        if addresses.is_empty() {
            Err(Error::Network)
        } else {
            Ok(addresses)
        }
    }

    // Decode only successful responses to the original DNS question, bounded to one DNS message.
    async fn query(&self, name: &Name, kind: RecordType) -> Result<Vec<IpAddr>> {
        let question = Query::query(name.clone(), kind);
        let mut message = Message::new();
        message
            .set_recursion_desired(true)
            .add_query(question.clone());
        let response = self
            .client
            .post(self.endpoint.clone())
            .header("content-type", "application/dns-message")
            .header("accept", "application/dns-message")
            .body(message.to_vec().map_err(|_| Error::Network)?)
            .send()
            .await
            .map_err(|_| Error::Network)?
            .error_for_status()
            .map_err(|_| Error::Network)?;
        let bytes = network::limited(response, usize::from(u16::MAX)).await?;
        let reply = Message::from_vec(&bytes).map_err(|_| Error::Network)?;
        if reply.message_type() != MessageType::Response
            || reply.id() != message.id()
            || reply.queries() != [question]
            || reply.response_code() != ResponseCode::NoError
        {
            return Err(Error::Network);
        }
        Ok(reply
            .answers()
            .iter()
            .filter_map(|record| match (kind, record.data()) {
                (RecordType::A, RData::A(address)) => Some(IpAddr::V4(address.0)),
                (RecordType::AAAA, RData::AAAA(address)) => Some(IpAddr::V6(address.0)),
                _ => None,
            })
            .collect())
    }
}

impl Resolve for Doh {
    fn resolve(&self, name: HttpName) -> Resolving {
        let resolver = self.clone();
        Box::pin(async move {
            let addresses = resolver.addresses(name.as_str(), 0).await?;
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

/// Public resolver replies must not introduce routes to private or special-purpose addresses.
fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, _, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || a == 0
                || a >= 224
                || (a == 100 && (64..128).contains(&b)))
        }
        IpAddr::V6(ip) => ip.segments()[0] & 0xe000 == 0x2000,
    }
}
