// Copyright (C) 2026 Impala Systems, Inc.
// SPDX-License-Identifier: AGPL-3.0-only

use super::*;

pub const MAX_RESEARCH_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

pub async fn fetch_url(args: &Value) -> Result<String> {
    let url = req_str(args, "url")?;
    let mut current_url = validate_public_http_url(url).await?;
    let max_chars = args
        .get("max_chars")
        .and_then(Value::as_u64)
        .unwrap_or(12_000)
        .clamp(1_000, 40_000) as usize;
    let mut redirects = 0usize;
    let response = loop {
        let client = http_client(&current_url)?;
        let response = client.get(current_url.url.clone()).send().await?;
        if !response.status().is_redirection() {
            break response;
        }
        if redirects >= 5 {
            return Err(anyhow!("too many redirects while fetching URL"));
        }
        let Some(location) = response.headers().get(reqwest::header::LOCATION) else {
            break response;
        };
        let location = location
            .to_str()
            .map_err(|err| anyhow!("redirect Location is not valid UTF-8: {err}"))?;
        current_url = validate_public_http_url(current_url.url.join(location)?.as_str()).await?;
        redirects += 1;
    };
    let status = response.status().as_u16();
    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let (body, body_truncated) =
        read_response_body_limited(response, MAX_RESEARCH_RESPONSE_BYTES).await?;
    let title = extract_title(&body);
    let visible_text = compact_visible_text(&body, max_chars);

    Ok(serde_json::to_string_pretty(&json!({
        "url": url,
        "finalUrl": final_url,
        "status": status,
        "contentType": content_type,
        "title": title,
        "text": visible_text,
        "truncated": body_truncated || body.len() > max_chars
    }))?)
}

pub async fn web_search(args: &Value) -> Result<String> {
    let query = req_str(args, "query")?;
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(5)
        .clamp(1, 10) as usize;
    let url = format!(
        "https://duckduckgo.com/html/?q={}",
        urlencoding::encode(query)
    );
    let url = validate_public_http_url(&url).await?;
    let response = http_client(&url)?.get(url.url.clone()).send().await?;
    let status = response.status().as_u16();
    let (body, _) = read_response_body_limited(response, MAX_RESEARCH_RESPONSE_BYTES).await?;
    let results = extract_duckduckgo_results(&body, limit);

    Ok(serde_json::to_string_pretty(&json!({
        "query": query,
        "provider": "duckduckgo-html",
        "status": status,
        "results": results,
        "citationPolicy": "Fetch a result URL with fetch_url before using it as evidence."
    }))?)
}

async fn read_response_body_limited(
    mut response: reqwest::Response,
    max_bytes: usize,
) -> Result<(String, bool)> {
    let mut bytes = Vec::new();
    let mut truncated = false;
    while let Some(chunk) = response.chunk().await? {
        if chunk.is_empty() {
            continue;
        }
        let remaining = max_bytes.saturating_sub(bytes.len());
        if remaining == 0 {
            truncated = true;
            break;
        }
        if chunk.len() > remaining {
            bytes.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    if truncated {
        trim_incomplete_utf8_tail(&mut bytes);
    }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}

fn trim_incomplete_utf8_tail(bytes: &mut Vec<u8>) {
    // Drop a partial trailing UTF-8 codepoint split by the byte cap so valid
    // UTF-8 pages don't end with U+FFFD replacement characters.
    if bytes.is_empty() {
        return;
    }
    let mut start = bytes.len() - 1;
    let min_start = bytes.len().saturating_sub(4);
    while start > min_start && (bytes[start] & 0b1100_0000) == 0b1000_0000 {
        start -= 1;
    }
    if let Err(err) = std::str::from_utf8(&bytes[start..]) {
        if err.error_len().is_none() {
            bytes.truncate(start);
        }
    }
}

pub fn http_client(target: &ValidatedHttpUrl) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("TalonResearchBot/0.1 (+https://impalasys.com)")
        .redirect(reqwest::redirect::Policy::none());
    if let Some(host) = target.host.as_deref() {
        builder = builder.resolve_to_addrs(host, &target.addrs);
    }
    builder
        .build()
        .map_err(|err| anyhow!("research HTTP client failed to build: {err}"))
}

pub fn validate_http_url(value: &str) -> Result<()> {
    let url = url::Url::parse(value).map_err(|err| anyhow!("invalid URL: {err}"))?;
    match url.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(anyhow!("unsupported URL scheme '{}'", scheme)),
    }
}

pub async fn validate_public_http_url(value: &str) -> Result<ValidatedHttpUrl> {
    validate_http_url(value)?;
    let url = url::Url::parse(value).map_err(|err| anyhow!("invalid URL: {err}"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("URL port is required"))?;

    let host = match url.host() {
        Some(url::Host::Ipv4(ip)) => {
            let ip = IpAddr::V4(ip);
            ensure_public_ip(ip)?;
            return Ok(ValidatedHttpUrl {
                url,
                host: None,
                addrs: vec![SocketAddr::new(ip, port)],
            });
        }
        Some(url::Host::Ipv6(ip)) => {
            let ip = IpAddr::V6(ip);
            ensure_public_ip(ip)?;
            return Ok(ValidatedHttpUrl {
                url,
                host: None,
                addrs: vec![SocketAddr::new(ip, port)],
            });
        }
        Some(url::Host::Domain(domain)) => domain.to_string(),
        None => return Err(anyhow!("URL host is required")),
    };

    let mut addrs = tokio::net::lookup_host((host.clone(), port))
        .await
        .map_err(|err| anyhow!("failed to resolve URL host '{host}': {err}"))?;
    let mut public_addrs = Vec::new();
    for addr in addrs.by_ref() {
        ensure_public_ip(addr.ip())?;
        public_addrs.push(addr);
    }
    if public_addrs.is_empty() {
        return Err(anyhow!("URL host '{host}' resolved no addresses"));
    }
    Ok(ValidatedHttpUrl {
        url,
        host: Some(host),
        addrs: public_addrs,
    })
}

pub fn ensure_public_ip(ip: IpAddr) -> Result<()> {
    let blocked = match ip {
        IpAddr::V4(ip) => is_blocked_ipv4(ip),
        IpAddr::V6(ip) => is_blocked_ipv6(ip),
    };
    if blocked {
        Err(anyhow!("URL resolves to a non-public address"))
    } else {
        Ok(())
    }
}

pub fn is_blocked_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || octets[0] == 0
        || octets[0] >= 224
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 169 && octets[1] == 254)
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
}

pub fn is_blocked_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_blocked_ipv4(v4);
    }

    let segments = ip.segments();

    // NAT64 well-known prefix (64:ff9b::/96, RFC 6052): inspect embedded IPv4.
    if segments[..6] == [0x0064, 0xff9b, 0, 0, 0, 0] {
        let embedded = Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        );
        return is_blocked_ipv4(embedded);
    }

    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        // Deprecated IPv4-compatible addresses (::/96)
        || segments[..6] == [0, 0, 0, 0, 0, 0]
        // IPv4-translated addresses (::ffff:0:0:0/96, RFC 2765)
        || segments[..6] == [0, 0, 0, 0, 0xffff, 0]
        // NAT64 local-use prefix (64:ff9b:1::/48, RFC 8215)
        || (segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2] == 0x0001)
        // Discard-only (100::/64, RFC 6666)
        || segments[..4] == [0x0100, 0, 0, 0]
        // Teredo (2001:0000::/32, RFC 4380)
        || (segments[0] == 0x2001 && segments[1] == 0x0000)
        // Benchmarking (2001:2::/48, RFC 5180)
        || (segments[0] == 0x2001 && segments[1] == 0x0002 && segments[2] == 0x0000)
        // ORCHID / ORCHIDv2 (2001:10::/28, 2001:20::/28)
        || (segments[0] == 0x2001
            && ((segments[1] & 0xfff0) == 0x0010 || (segments[1] & 0xfff0) == 0x0020))
        // Documentation (2001:db8::/32, RFC 3849)
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        // 6to4 (2002::/16, RFC 3056 / deprecated by RFC 7526)
        || segments[0] == 0x2002
        // Unique-local (fc00::/7)
        || (segments[0] & 0xfe00) == 0xfc00
        // Link-local (fe80::/10)
        || (segments[0] & 0xffc0) == 0xfe80
        // Deprecated site-local (fec0::/10)
        || (segments[0] & 0xffc0) == 0xfec0
}

pub fn extract_title(html: &str) -> String {
    let lower = html.to_lowercase();
    let Some(start) = lower.find("<title") else {
        return String::new();
    };
    let Some(open_end) = lower[start..].find('>') else {
        return String::new();
    };
    let content_start = start + open_end + 1;
    let Some(close) = lower[content_start..].find("</title>") else {
        return String::new();
    };
    decode_html_entities(&html[content_start..content_start + close])
        .trim()
        .to_string()
}

pub fn compact_visible_text(input: &str, max_chars: usize) -> String {
    let without_scripts = remove_tag_blocks(input, "script");
    let without_styles = remove_tag_blocks(&without_scripts, "style");
    let mut text = String::with_capacity(without_styles.len().min(max_chars));
    let mut in_tag = false;
    let mut last_was_space = true;
    for ch in without_styles.chars() {
        match ch {
            '<' => {
                in_tag = true;
                if !last_was_space {
                    text.push(' ');
                    last_was_space = true;
                }
            }
            '>' => in_tag = false,
            _ if in_tag => {}
            _ if ch.is_whitespace() => {
                if !last_was_space {
                    text.push(' ');
                    last_was_space = true;
                }
            }
            _ => {
                text.push(ch);
                last_was_space = false;
            }
        }
        if text.len() >= max_chars {
            break;
        }
    }
    decode_html_entities(text.trim())
}

pub fn remove_tag_blocks(input: &str, tag: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let lower = input.to_lowercase();
    let open_prefix = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let mut pos = 0;
    while let Some(start_rel) = lower[pos..].find(&open_prefix) {
        let start = pos + start_rel;
        output.push_str(&input[pos..start]);
        if let Some(end_rel) = lower[start..].find(&close) {
            pos = start + end_rel + close.len();
        } else {
            pos = input.len();
            break;
        }
    }
    output.push_str(&input[pos..]);
    output
}

pub fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

pub fn extract_duckduckgo_results(html: &str, limit: usize) -> Vec<Value> {
    let mut results: Vec<Value> = Vec::new();
    let mut pos = 0;
    while results.len() < limit {
        let Some(class_rel) = html[pos..].find("result__a") else {
            break;
        };
        let class_pos = pos + class_rel;
        let anchor_start = html[..class_pos].rfind("<a").unwrap_or(class_pos);
        let Some(anchor_end_rel) = html[class_pos..].find("</a>") else {
            break;
        };
        let anchor_end = class_pos + anchor_end_rel + "</a>".len();
        let anchor = &html[anchor_start..anchor_end];
        let Some(href) = extract_attr(anchor, "href") else {
            pos = anchor_end;
            continue;
        };
        let Some(url) = normalize_search_result_url(&href) else {
            pos = anchor_end;
            continue;
        };
        let title = compact_visible_text(anchor, 500);
        if !title.is_empty() && !results.iter().any(|item| item["url"] == url) {
            results.push(json!({
                "title": title,
                "url": url
            }));
        }
        pos = anchor_end;
    }
    results
}

pub fn extract_attr(input: &str, attr: &str) -> Option<String> {
    let needle = format!("{}=\"", attr);
    let start = input.find(&needle)? + needle.len();
    let end = input[start..].find('"')?;
    Some(decode_html_entities(&input[start..start + end]))
}

pub fn normalize_search_result_url(href: &str) -> Option<String> {
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href.to_string());
    }
    let query_start = href.find("uddg=")? + "uddg=".len();
    let query = &href[query_start..];
    let value = query.split('&').next().unwrap_or(query);
    urlencoding::decode(value)
        .ok()
        .map(|value| value.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, routing::get, Router};
    use futures::stream;
    use tokio::net::TcpListener;

    #[test]
    fn is_blocked_ipv6_blocks_ipv4_mapped_and_special_ranges() {
        let blocked = [
            "::1",
            "::",
            "::ffff:169.254.169.254",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:172.16.0.1",
            "::ffff:192.168.1.1",
            "::ffff:100.64.0.1",
            "::ffff:0.0.0.0",
            "::169.254.169.254",
            "::127.0.0.1",
            "::ffff:0:169.254.169.254",
            "64:ff9b::169.254.169.254",
            "64:ff9b::127.0.0.1",
            "64:ff9b::10.0.0.1",
            "64:ff9b:1::1",
            "100::1",
            "2001::1",
            "2001:2::1",
            "2001:10::1",
            "2001:20::1",
            "2001:db8::1",
            "2002:a9fe:a9fe::1",
            "fc00::1",
            "fd12:3456:789a::1",
            "fe80::1",
            "fec0::1",
            "ff02::1",
        ];
        for raw in blocked {
            let ip: Ipv6Addr = raw
                .parse()
                .unwrap_or_else(|e| panic!("bad test ip {raw}: {e}"));
            assert!(is_blocked_ipv6(ip), "expected IPv6 {raw} to be blocked");
        }

        let allowed = [
            "2606:2800:220:1:248:1893:25c8:1946",
            "2001:4860:4860::8888",
            "::ffff:93.184.216.34",
            "64:ff9b::93.184.216.34",
        ];
        for raw in allowed {
            let ip: Ipv6Addr = raw
                .parse()
                .unwrap_or_else(|e| panic!("bad test ip {raw}: {e}"));
            assert!(!is_blocked_ipv6(ip), "expected IPv6 {raw} to be allowed");
        }
    }

    #[tokio::test]
    async fn validate_public_http_url_rejects_bracketed_ipv6_ssrf_literals() {
        let blocked_urls = [
            "http://[::ffff:169.254.169.254]/latest/meta-data/",
            "http://[::ffff:127.0.0.1]:8081/pubsub/push",
            "http://[::ffff:10.0.0.1]/secret",
            "http://[::1]:8081/pubsub/push",
            "http://[64:ff9b::169.254.169.254]/",
            "http://[2002:a9fe:a9fe::1]/",
            "http://[2001:db8::1]/",
            "http://169.254.169.254/latest/meta-data/",
            "http://127.0.0.1:8081/pubsub/push",
        ];
        for url in blocked_urls {
            let err = validate_public_http_url(url)
                .await
                .expect_err(&format!("expected URL {url} to be rejected"));
            assert!(
                err.to_string().contains("non-public address"),
                "unexpected error for {url}: {err}"
            );
        }

        let public = validate_public_http_url("http://[2606:2800:220:1:248:1893:25c8:1946]/")
            .await
            .expect("public IPv6 literal should be allowed");
        assert!(public.host.is_none());
        assert_eq!(public.addrs.len(), 1);
    }

    #[tokio::test]
    async fn read_response_body_limited_caps_oversized_streaming_responses() {
        let app = Router::new()
            .route(
                "/huge",
                get(|| async {
                    let chunks = (0..64).map(|_| Ok::<_, std::io::Error>(vec![b'a'; 1024]));
                    Body::from_stream(stream::iter(chunks))
                }),
            )
            .route(
                "/utf8-boundary",
                get(|| async { "ééééé" }), // 5 * 2 bytes = 10 bytes
            );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = reqwest::Client::new();

        let huge_resp = client
            .get(format!("http://{addr}/huge"))
            .send()
            .await
            .unwrap();
        let (body, truncated) = read_response_body_limited(huge_resp, 4096).await.unwrap();
        assert!(truncated);
        assert_eq!(body.len(), 4096);
        assert!(body.bytes().all(|b| b == b'a'));

        let utf8_resp = client
            .get(format!("http://{addr}/utf8-boundary"))
            .send()
            .await
            .unwrap();
        // Odd byte cap splits a 2-byte UTF-8 sequence; tail should be trimmed cleanly.
        let (utf8_body, utf8_truncated) = read_response_body_limited(utf8_resp, 5).await.unwrap();
        assert!(utf8_truncated);
        assert_eq!(utf8_body, "éé");

        let exact_resp = client
            .get(format!("http://{addr}/utf8-boundary"))
            .send()
            .await
            .unwrap();
        let (exact_body, exact_truncated) =
            read_response_body_limited(exact_resp, 1024).await.unwrap();
        assert!(!exact_truncated);
        assert_eq!(exact_body, "ééééé");

        server.abort();
    }
}
