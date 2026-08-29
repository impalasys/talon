use super::*;


pub(crate) async fn fetch_url(args: &Value) -> Result<String> {
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
    let body = response.text().await?;
    let title = extract_title(&body);
    let visible_text = compact_visible_text(&body, max_chars);

    Ok(serde_json::to_string_pretty(&json!({
        "url": url,
        "finalUrl": final_url,
        "status": status,
        "contentType": content_type,
        "title": title,
        "text": visible_text,
        "truncated": body.len() > max_chars
    }))?)
}

pub(crate) async fn web_search(args: &Value) -> Result<String> {
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
    let body = response.text().await?;
    let results = extract_duckduckgo_results(&body, limit);

    Ok(serde_json::to_string_pretty(&json!({
        "query": query,
        "provider": "duckduckgo-html",
        "status": status,
        "results": results,
        "citationPolicy": "Fetch a result URL with fetch_url before using it as evidence."
    }))?)
}

pub(crate) fn http_client(target: &ValidatedHttpUrl) -> Result<reqwest::Client> {
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

pub(crate) fn validate_http_url(value: &str) -> Result<()> {
    let url = url::Url::parse(value).map_err(|err| anyhow!("invalid URL: {err}"))?;
    match url.scheme() {
        "http" | "https" => Ok(()),
        scheme => Err(anyhow!("unsupported URL scheme '{}'", scheme)),
    }
}

pub(crate) async fn validate_public_http_url(value: &str) -> Result<ValidatedHttpUrl> {
    validate_http_url(value)?;
    let url = url::Url::parse(value).map_err(|err| anyhow!("invalid URL: {err}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("URL host is required"))?
        .to_string();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("URL port is required"))?;

    if let Ok(ip) = host.parse::<IpAddr>() {
        ensure_public_ip(ip)?;
        return Ok(ValidatedHttpUrl {
            url,
            host: None,
            addrs: vec![SocketAddr::new(ip, port)],
        });
    }

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

pub(crate) fn ensure_public_ip(ip: IpAddr) -> Result<()> {
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

pub(crate) fn is_blocked_ipv4(ip: Ipv4Addr) -> bool {
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
}

pub(crate) fn is_blocked_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (ip.segments()[0] & 0xfe00) == 0xfc00
        || (ip.segments()[0] & 0xffc0) == 0xfe80
}

pub(crate) fn extract_title(html: &str) -> String {
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

pub(crate) fn compact_visible_text(input: &str, max_chars: usize) -> String {
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

pub(crate) fn remove_tag_blocks(input: &str, tag: &str) -> String {
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

pub(crate) fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

pub(crate) fn extract_duckduckgo_results(html: &str, limit: usize) -> Vec<Value> {
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

pub(crate) fn extract_attr(input: &str, attr: &str) -> Option<String> {
    let needle = format!("{}=\"", attr);
    let start = input.find(&needle)? + needle.len();
    let end = input[start..].find('"')?;
    Some(decode_html_entities(&input[start..start + end]))
}

pub(crate) fn normalize_search_result_url(href: &str) -> Option<String> {
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
