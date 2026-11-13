use crate::i18n::{gettext, gettext_f, gettext_noop};

use reqwest::{
    StatusCode,
    header::{ACCEPT, ACCEPT_ENCODING, CONTENT_TYPE, REFERER, USER_AGENT},
};
use scraper::{Html, Selector};
use url::Url;

use super::{WebSource, extract, network};
use crate::providers::policy::NetworkPolicy;

const SEARCH_URL: &str = "https://html.duckduckgo.com/html/";
pub(super) const BLOCKED: &str = gettext_noop(
    "DuckDuckGo is temporarily limiting searches or requires verification. Try again later or choose another search provider in Preferences → Web.",
);
const UNREADABLE: &str = gettext_noop(
    "DuckDuckGo returned an unreadable search page. Try another search provider in Preferences → Web.",
);

pub(super) async fn search(query: &str, policy: &NetworkPolicy) -> Result<Vec<WebSource>, String> {
    let url = network::public_url(SEARCH_URL)?;
    let client = network::client(&url, policy).await?;
    network::check_policy(policy)?;
    let response = client
        .post(url)
        .header(ACCEPT, "text/html, application/xhtml+xml;q=0.9")
        .header(ACCEPT_ENCODING, "identity")
        .header(REFERER, "https://html.duckduckgo.com/")
        .header(
            USER_AGENT,
            concat!(
                "Mozilla/5.0 (compatible; Moose/",
                env!("CARGO_PKG_VERSION"),
                "; +https://github.com/moooossee/moose)"
            ),
        )
        .form(&[("q", query), ("b", ""), ("kl", "wt-wt")])
        .send()
        .await
        .map_err(|_| gettext("DuckDuckGo could not be reached. Try again later."))?;
    network::check_policy(policy)?;
    if matches!(
        response.status(),
        StatusCode::ACCEPTED | StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
    ) {
        return Err(BLOCKED.into());
    }
    if response.status().is_redirection() {
        return Err(gettext(
            "DuckDuckGo redirected the search. Try a query without !bang shortcuts or choose another search provider in Preferences → Web.",
        ));
    }
    if !response.status().is_success() {
        return Err(gettext_f(
            "DuckDuckGo returned HTTP {arg1}. Try again later or choose another search provider in Preferences → Web.",
            &[("arg1", &(response.status().as_u16()).to_string())],
        ));
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(
        content_type.split(';').next().unwrap_or("").trim(),
        "text/html" | "application/xhtml+xml"
    ) {
        return Err(UNREADABLE.into());
    }
    let bytes = network::body(response, policy).await?;
    let sources = tokio::task::spawn_blocking(move || parse(&bytes))
        .await
        .map_err(|_| gettext("DuckDuckGo result extraction was interrupted."))??;
    network::check_policy(policy)?;
    Ok(sources)
}

fn parse(bytes: &[u8]) -> Result<Vec<WebSource>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| UNREADABLE)?;
    let document = Html::parse_document(text);
    let challenge =
        selector("#challenge-form, #img-form, form[action*='anomaly.js'], .anomaly-modal")?;
    if document.select(&challenge).next().is_some() {
        return Err(BLOCKED.into());
    }
    let results = selector(".web-result:not(.result--ad), .result.results_links:not(.result--ad)")?;
    let link = selector("a.result__a, h2 a")?;
    let snippet = selector(".result__snippet")?;
    let base = Url::parse(SEARCH_URL).map_err(|_| UNREADABLE)?;
    let mut sources: Vec<WebSource> = Vec::new();
    for result in document.select(&results).take(30) {
        let Some(link) = result.select(&link).next() else {
            continue;
        };
        let Some(url) = link
            .value()
            .attr("href")
            .and_then(|href| result_url(href, &base))
        else {
            continue;
        };
        if sources.iter().any(|source| source.url == url.as_str()) {
            continue;
        }
        let title = extract::clean(&link.text().collect::<Vec<_>>().join(" "), 240);
        if title.is_empty() {
            continue;
        }
        let content = result
            .select(&snippet)
            .next()
            .map(|element| extract::clean(&element.text().collect::<Vec<_>>().join(" "), 600))
            .unwrap_or_default();
        sources.push(WebSource {
            id: String::new(),
            title,
            url: url.into(),
            content,
            page_read: false,
            truncated: false,
            retrieved_at: crate::core::utc_now(),
        });
        if sources.len() == 5 {
            break;
        }
    }
    if sources.is_empty() {
        let empty = selector(".no-results, .no-results__message, .result--no-result")?;
        if document.select(&empty).next().is_none() {
            return Err(UNREADABLE.into());
        }
    }
    Ok(sources)
}

fn result_url(href: &str, base: &Url) -> Option<Url> {
    let url = base.join(href).ok()?;
    if matches!(
        url.host_str(),
        Some("duckduckgo.com" | "www.duckduckgo.com" | "html.duckduckgo.com")
    ) && matches!(url.path(), "/l" | "/l/")
    {
        let target = url
            .query_pairs()
            .find(|(name, _)| name.as_ref() == "uddg")?
            .1;
        network::public_url(&target).ok()
    } else {
        network::public_url(url.as_str()).ok()
    }
}

fn selector(value: &str) -> Result<Selector, String> {
    Selector::parse(value).map_err(|_| gettext("DuckDuckGo result extraction is unavailable."))
}
