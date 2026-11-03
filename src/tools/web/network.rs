use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use futures_util::StreamExt;
use reqwest::{
    Client, Response,
    header::{ACCEPT, ACCEPT_ENCODING, CONTENT_TYPE, LOCATION},
};
use url::{Host, Url};

use crate::providers::policy::NetworkPolicy;

pub const MAX_BODY: usize = 1024 * 1024;

pub fn check_policy(policy: &NetworkPolicy) -> Result<(), String> {
    if policy.local_only() {
        Err("Web access is blocked by Local Only. Turn it off in Preferences → Privacy to use the web.".into())
    } else {
        Ok(())
    }
}

pub fn public_url(value: &str) -> Result<Url, String> {
    let mut url = Url::parse(value).map_err(|_| "Enter a complete public HTTP or HTTPS URL.")?;
    if value.len() > 2048
        || !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.port_or_known_default(), Some(80 | 443))
    {
        return Err(
            "Only public HTTP and HTTPS pages without credentials on ports 80 or 443 can be read."
                .into(),
        );
    }
    match url.host() {
        Some(Host::Ipv4(ip)) if public_ip(IpAddr::V4(ip)) => {}
        Some(Host::Ipv6(ip)) if public_ip(IpAddr::V6(ip)) => {}
        Some(Host::Domain(host))
            if host.contains('.')
                && !host.ends_with('.')
                && ![
                    "localhost",
                    "local",
                    "internal",
                    "home",
                    "lan",
                    "onion",
                    "test",
                    "invalid",
                ]
                .iter()
                .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}"))) => {}
        _ => return Err("Local, private and reserved network addresses cannot be read.".into()),
    }
    url.set_fragment(None);
    Ok(url)
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(matches!(a, 0 | 10 | 127 | 224..=255)
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && (b == 168 || b == 0 || (b == 88 && c == 99)))
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            (s[0] & 0xe000) == 0x2000
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && s[0] != 0x2002
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}

pub async fn client(url: &Url, policy: &NetworkPolicy) -> Result<Client, String> {
    check_policy(policy)?;
    let host = url.host_str().ok_or("The URL has no host.")?;
    let port = url.port_or_known_default().ok_or("The URL has no port.")?;
    let addresses: Vec<SocketAddr> = match url.host() {
        Some(Host::Ipv4(ip)) => vec![SocketAddr::new(ip.into(), port)],
        Some(Host::Ipv6(ip)) => vec![SocketAddr::new(ip.into(), port)],
        _ => tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| "The website address could not be resolved.")?
            .take(32)
            .collect(),
    };
    if addresses.is_empty() || addresses.iter().any(|address| !public_ip(address.ip())) {
        return Err("Local, private and reserved network addresses cannot be accessed.".into());
    }
    check_policy(policy)?;
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(25))
        .user_agent(concat!("Moose/", env!("CARGO_PKG_VERSION")))
        .resolve_to_addrs(host, &addresses)
        .build()
        .map_err(|_| "The web connection could not be prepared.".into())
}

pub async fn body(response: Response, policy: &NetworkPolicy) -> Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BODY as u64)
    {
        return Err("The page exceeds the 1 MB download limit.".into());
    }
    let mut data = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        check_policy(policy)?;
        let chunk = chunk.map_err(|_| "The website stopped responding.")?;
        if data.len().saturating_add(chunk.len()) > MAX_BODY {
            return Err("The page exceeds the 1 MB download limit.".into());
        }
        data.extend_from_slice(&chunk);
    }
    check_policy(policy)?;
    Ok(data)
}

pub async fn page(value: &str, policy: &NetworkPolicy) -> Result<(Url, String, Vec<u8>), String> {
    let mut url = public_url(value)?;
    for redirect in 0..=4 {
        let client = client(&url, policy).await?;
        check_policy(policy)?;
        let response = client
            .get(url.clone())
            .header(ACCEPT, "text/html, text/plain;q=0.9")
            .header(ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(|_| "The page could not be reached. Try another source.")?;
        check_policy(policy)?;
        if response.status().is_redirection() {
            if redirect == 4 {
                return Err("The website redirected too many times.".into());
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or("The website returned an invalid redirect.")?;
            let next = url
                .join(location)
                .map_err(|_| "The website returned an invalid redirect.")?;
            if url.scheme() == "https" && next.scheme() != "https" {
                return Err("The website redirected to an insecure connection.".into());
            }
            url = public_url(next.as_str())?;
            continue;
        }
        if !response.status().is_success() {
            return Err(format!(
                "The website returned HTTP {}. Try another source.",
                response.status().as_u16()
            ));
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mime = content_type.split(';').next().unwrap_or("").trim();
        if !matches!(mime, "text/html" | "application/xhtml+xml" | "text/plain") {
            return Err("This link is not an HTML or plain-text page. Downloads, images and PDFs are not supported by Web.".into());
        }
        let bytes = body(response, policy).await?;
        return Ok((url, content_type, bytes));
    }
    Err("The page could not be read.".into())
}
