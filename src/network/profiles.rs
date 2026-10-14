use crate::core::{Result, hash, invalid};
use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub label: String,
    pub url: String,
}
impl Profile {
    pub fn sanitized(&self) -> serde_json::Value {
        let parsed = Url::parse(&self.url).ok();
        serde_json::json!({"id":self.id,"label":self.label,"scheme":parsed.as_ref().map(Url::scheme),"host":parsed.as_ref().and_then(Url::host_str),"port":parsed.as_ref().and_then(Url::port),"credentials_present":parsed.is_some_and(|u|!u.username().is_empty())})
    }
}
pub fn normalize(value: &str) -> Result<Url> {
    if value.len() > 2048 || value.chars().any(|c| c.is_control()) {
        return Err(invalid(
            "Proxy URL exceeds its limit or contains control characters.",
        ));
    }
    let mut url = Url::parse(value)
        .map_err(|_| invalid("Malformed proxy URL; use an explicit HTTP, HTTPS or SOCKS5h URL."))?;
    if !["http", "https", "socks5", "socks5h"].contains(&url.scheme()) {
        return Err(invalid(
            "Supported proxy schemes are HTTP, HTTPS and SOCKS5/SOCKS5h. SOCKS4 inputs are refused, not silently weakened.",
        ));
    }
    if url.host_str().is_none()
        || url.query().is_some()
        || url.fragment().is_some()
        || (!url.path().is_empty() && url.path() != "/")
    {
        return Err(invalid(
            "Proxy must contain a host/port and optional encoded credentials, without a path/query/fragment.",
        ));
    }
    let port = url.port().unwrap_or(match url.scheme() {
        "https" => 443,
        "http" => 80,
        _ => 1080,
    });
    if port == 0 {
        return Err(invalid("Proxy port must be 1-65535."));
    }
    url.set_port(Some(port))
        .map_err(|_| invalid("Invalid proxy port."))?;
    if url.scheme() == "socks5" {
        url.set_scheme("socks5h")
            .map_err(|_| invalid("Cannot enforce remote destination DNS."))?;
    }
    let user = percent_decode_str(url.username())
        .decode_utf8()
        .map_err(|_| invalid("Proxy credentials are not UTF-8."))?;
    let pass = percent_decode_str(url.password().unwrap_or(""))
        .decode_utf8()
        .map_err(|_| invalid("Proxy credentials are not UTF-8."))?;
    if user.contains(['\0', '\r', '\n'])
        || pass.contains(['\0', '\r', '\n'])
        || user.len() > 255
        || pass.len() > 255
    {
        return Err(invalid(
            "Proxy credentials exceed protocol limits or contain forbidden characters.",
        ));
    }
    if url.password().is_some() && user.is_empty() {
        return Err(invalid("Proxy password requires a username."));
    }
    Ok(url)
}
fn named_line(line: &str) -> Result<Option<(String, String)>> {
    let Some((scheme, tail)) = line.split_once("://") else {
        return Ok(None);
    };
    if tail.contains('@') {
        return Ok(None);
    }
    let (host, rest) = if tail.starts_with('[') {
        let Some(end) = tail.find(']') else {
            return Err(invalid("Invalid IPv6 proxy host."));
        };
        (
            &tail[..=end],
            tail[end + 1..].strip_prefix(':').unwrap_or(""),
        )
    } else {
        let Some((host, rest)) = tail.split_once(':') else {
            return Ok(None);
        };
        (host, rest)
    };
    let fields: Vec<_> = rest.splitn(4, ':').collect();
    if fields.len() != 4 {
        return Ok(None);
    }
    let encoded = |s: &str| {
        percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
    };
    let auth = if fields[1].is_empty() && fields[2].is_empty() {
        String::new()
    } else {
        format!("{}:{}@", encoded(fields[1]), encoded(fields[2]))
    };
    Ok(Some((
        fields[3].trim().to_owned(),
        format!("{scheme}://{auth}{host}:{}", fields[0]),
    )))
}
pub fn parse_import(text: &str) -> Result<Vec<Profile>> {
    if text.len() > 256 * 1024 {
        return Err(invalid("Proxy import exceeds 256 KiB."));
    }
    let mut profiles = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut labels = std::collections::HashSet::new();
    for (index, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if profiles.len() >= 100 {
            return Err(invalid("Proxy import contains more than 100 profiles."));
        }
        let parsed = (|| {
            let (label, value) = if let Some((label, url)) = line.split_once('|') {
                (label.trim().to_string(), url.trim().to_string())
            } else if let Some(named) = named_line(line)? {
                named
            } else {
                (format!("Proxy {}", index + 1), line.to_string())
            };
            crate::core::bounded_text(&label, 80)?;
            if label.chars().any(char::is_control) {
                return Err(invalid("Proxy label contains control characters."));
            }
            if label.is_empty() {
                return Err(invalid("Proxy label is empty."));
            }
            let value = if value.contains("://") {
                value
            } else {
                let parts: Vec<_> = value.split(':').collect();
                if parts.len() != 4 || parts[0].contains(['[', ']']) {
                    return Err(invalid(
                        "Legacy import requires host:port:user:password; use a standard URL for IPv6 or delimiters.",
                    ));
                }
                let encoded = |s: &str| {
                    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC)
                        .to_string()
                };
                format!(
                    "socks5h://{}:{}@{}:{}",
                    encoded(parts[2]),
                    encoded(parts[3]),
                    parts[0],
                    parts[1]
                )
            };
            Ok((label, normalize(&value)?.to_string()))
        })();
        let (label,url)=parsed.map_err(|_|invalid(format!("Proxy import line {} is invalid; no profiles imported. Use an explicit encoded URL.",index+1)))?;
        let identity = hash(url.as_bytes());
        if !seen.insert(identity) {
            continue;
        }
        if !labels.insert(label.to_ascii_lowercase()) {
            return Err(invalid(format!(
                "Duplicate proxy label at line {}; import cancelled.",
                index + 1
            )));
        }
        profiles.push(Profile {
            id: uuid::Uuid::new_v4().to_string(),
            label,
            url,
        });
    }
    Ok(profiles)
}
