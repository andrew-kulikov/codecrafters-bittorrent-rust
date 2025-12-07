use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Ok};
use reqwest::Url;

/// Parsed representation of a BitTorrent magnet URI.
#[derive(Debug, Clone)]
pub struct MagnetLink {
    /// All exact-topic identifiers (e.g. btih, btmh).
    pub exact_topics: Vec<ExactTopic>,

    /// Display name (`dn`).
    pub display_name: Option<String>,

    /// Trackers (`tr`).
    pub trackers: Vec<Url>,

    /// Web seeds (`ws`).
    pub web_seeds: Vec<Url>,

    /// Acceptable sources (`as`).
    pub acceptable_sources: Vec<Url>,

    /// Exact sources (`xs`).
    pub exact_sources: Vec<Url>,

    /// Explicit peers (`x.pe`).
    pub peers: Vec<PeerAddress>,

    /// Exact length in bytes (`xl`).
    pub length: Option<u64>,

    /// Selected file indices from `so` (BEP-53), if present.
    pub select_only: Option<Vec<u32>>,

    /// Any parameters we didn’t explicitly interpret.
    pub other_params: HashMap<String, Vec<String>>,
}

/// `xt` values we care about.
#[derive(Debug, Clone)]
pub enum ExactTopic {
    /// `xt=urn:btih:<infohash>` (SHA-1 v1 infohash, hex or base32 string).
    Btih(String),

    /// `xt=urn:btmh:<tagged-infohash>` (v2 “multihash”).
    Btmh(String),

    /// Something else (`urn:sha1:…`, other networks, etc).
    Other(String),
}

impl ExactTopic {
    /// Get the raw infohash string, if this is a btih or btmh topic.
    pub fn get_hash(&self) -> Option<&str> {
        match self {
            ExactTopic::Btih(s) | ExactTopic::Btmh(s) => Some(s),
            ExactTopic::Other(_) => None,
        }
    }
}

/// Bootstrap peer address from `x.pe`.
#[derive(Debug, Clone)]
pub struct PeerAddress {
    pub host: String,
    pub port: u16,
}

impl MagnetLink {
    /// Parse a magnet URI into a `MagnetLink`.
    pub fn parse(input: &str) -> anyhow::Result<MagnetLink> {
        let url = Url::parse(input).context("invalid URL")?;

        if url.scheme() != "magnet" {
            bail!("expected magnet: scheme, got {}", url.scheme());
        }

        let query = url.query_pairs();

        // Collect all query parameters into a `HashMap<key, Vec<value>>`
        let mut params: HashMap<String, Vec<String>> = HashMap::new();
        for (k, v) in query {
            params
                .entry(k.into_owned())
                .or_default()
                .push(v.into_owned());
        }

        // ----- xt (required, may appear multiple times) -----
        let xt_values = params
            .remove("xt")
            .ok_or_else(|| anyhow!("magnet link must contain at least one xt parameter"))?;

        let exact_topics = xt_values
            .into_iter()
            .map(|xt| parse_exact_topic(&xt))
            .collect::<anyhow::Result<Vec<_>>>()?;

        // ----- dn (optional, we just take the last one if several) -----
        let display_name = params.get("dn").and_then(|list| list.last().cloned());

        // ----- trackers (tr) -----
        let trackers =
            collect_urls(params.remove("tr")).context("failed to parse tracker URLs (tr)")?;

        // ----- web seeds (ws) -----
        let web_seeds =
            collect_urls(params.remove("ws")).context("failed to parse web seed URLs (ws)")?;

        // ----- acceptable sources (as) -----
        let acceptable_sources = collect_urls(params.remove("as"))
            .context("failed to parse acceptable-source URLs (as)")?;

        // ----- exact sources (xs) -----
        let exact_sources =
            collect_urls(params.remove("xs")).context("failed to parse exact-source URLs (xs)")?;

        // ----- explicit peers (x.pe) -----
        let peers = match params.remove("x.pe") {
            Some(values) => {
                let mut peers = Vec::new();
                for v in values {
                    peers.push(parse_peer(&v).with_context(|| format!("invalid x.pe value: {v}"))?);
                }
                peers
            }
            None => Vec::new(),
        };

        // ----- exact length (xl) -----
        let length = params
            .remove("xl")
            .and_then(|values| values.last().cloned())
            .map(|s| s.parse::<u64>())
            .transpose()
            .context("invalid xl value (expected u64)")?;

        // ----- select-only (so, BEP-53) -----
        let select_only = match params
            .remove("so")
            .and_then(|values| values.last().cloned())
        {
            Some(value) => Some(parse_select_only(&value)?),
            None => None,
        };

        // Anything we didn’t explicitly interpret stays in other_params.
        let other_params = params;

        Ok(MagnetLink {
            exact_topics,
            display_name,
            trackers,
            web_seeds,
            acceptable_sources,
            exact_sources,
            peers,
            length,
            select_only,
            other_params,
        })
    }
}

/// Parse a single `xt=...` into `ExactTopic`.
fn parse_exact_topic(xt: &str) -> anyhow::Result<ExactTopic> {
    const BTIH_PREFIX: &str = "urn:btih:";
    const BTMH_PREFIX: &str = "urn:btmh:";

    if let Some(rest) = xt.strip_prefix(BTIH_PREFIX) {
        Ok(ExactTopic::Btih(rest.to_string()))
    } else if let Some(rest) = xt.strip_prefix(BTMH_PREFIX) {
        Ok(ExactTopic::Btmh(rest.to_string()))
    } else {
        // Keep other URNs as-is.
        Ok(ExactTopic::Other(xt.to_string()))
    }
}

/// Collect a list of `String` values into `Url`s.
/// If `values` is `None`, returns an empty vec.
fn collect_urls(values: Option<Vec<String>>) -> anyhow::Result<Vec<Url>> {
    let mut urls = Vec::new();
    if let Some(values) = values {
        for raw in values {
            let url = Url::parse(&raw).with_context(|| format!("invalid URL: {raw}"))?;
            urls.push(url);
        }
    }
    Ok(urls)
}

/// Parse `x.pe` string into a `PeerAddress`.
///
/// Expected formats:
/// - `host:port`
/// - `1.2.3.4:51413`
/// - `[2001:db8::1]:51413` (host may contain `:` / brackets; we split on last `:`).
fn parse_peer(value: &str) -> anyhow::Result<PeerAddress> {
    let (host, port_str) = value
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("x.pe value must be host:port"))?;

    let port: u16 = port_str.parse().context("invalid port in x.pe value")?;

    Ok(PeerAddress {
        host: host.to_string(),
        port,
    })
}

/// Parse `so` (BEP-53) into a list of file indices.
///
/// Example:
/// - `so=0,2,4,6-8` → [0,2,4,6,7,8]
fn parse_select_only(value: &str) -> anyhow::Result<Vec<u32>> {
    let mut result = Vec::new();

    for part in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Some((start_str, end_str)) = part.split_once('-') {
            let start: u32 = start_str
                .parse()
                .with_context(|| format!("invalid start index in so range: {part}"))?;
            let end: u32 = end_str
                .parse()
                .with_context(|| format!("invalid end index in so range: {part}"))?;

            if start > end {
                bail!("so range start > end: {part}");
            }

            // Expand range.
            result.extend(start..=end);
        } else {
            let idx: u32 = part
                .parse()
                .with_context(|| format!("invalid index in so: {part}"))?;
            result.push(idx);
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_magnet_link() {
        let magnet_link = "magnet:?xt=urn:btih:abcdef1234567890abcdef1234567890abcdef12&dn=example_file.txt&tr=http%3A%2F%2Ftracker.example.com%2Fannounce";
        let result = MagnetLink::parse(magnet_link);
        assert!(result.is_ok());
    }

    #[test]
    fn parse_magnet_link_with_unknown_xt() {
        let magnet_link = "magnet:?xt=urn:invalid:abcdef1234567890abcdef1234567890abcdef12&dn=example_file.txt&tr=http%3A%2F%2Ftracker.example.com%2Fannounce";
        let magnet = MagnetLink::parse(magnet_link).unwrap();
        assert!(match &magnet.exact_topics[0] {
            ExactTopic::Other(_) => true,
            _ => false,
        });
    }

    #[test]
    fn parse_magnet_link_missing_params() {
        let magnet_link =
            "magnet:?dn=example_file.txt&tr=http%3A%2F%2Ftracker.example.com%2Fannounce";
        let result = MagnetLink::parse(magnet_link);
        assert!(result.is_err());
    }

    #[test]
    fn parse_magnet_link_multiple_xt() {
        let magnet_link = "magnet:?xt=urn:btih:abcdef1234567890abcdef1234567890abcdef12&xt=urn:btih:1234567890abcdef1234567890abcdef12345678&dn=example_file.txt&tr=http%3A%2F%2Ftracker.example.com%2Fannounce";
        let result = MagnetLink::parse(magnet_link).unwrap();
        assert_eq!(result.exact_topics.len(), 2);
    }
}
