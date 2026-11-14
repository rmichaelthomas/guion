//! HTTP client plumbing for the native host.

use super::*;

#[derive(Debug)]
pub(crate) struct HostHttpClient {
    client: reqwest::Client,
    executor: tokio::runtime::Runtime,
}

impl HostHttpClient {
    pub(crate) fn new(max_parallelism: usize) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| error.to_string())?;
        let executor = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(max_parallelism.clamp(1, 8))
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { client, executor })
    }
}

#[derive(Debug)]
pub(crate) struct HttpBytesResponse {
    pub(crate) status: u16,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Vec<u8>,
}

impl HttpBytesResponse {
    pub(crate) fn into_text_json(
        self,
        max_text_bytes: usize,
    ) -> Result<serde_json::Value, HttpErrorValue> {
        if self.body.len() > max_text_bytes {
            return Err(http_error(
                "R_HTTP_LIMIT",
                "HTTP response exceeds the configured text byte limit",
            ));
        }
        let body = String::from_utf8(self.body)
            .map_err(|_| http_error("R_HTTP_ENCODING", "HTTP response body is not valid UTF-8"))?;
        Ok(serde_json::json!({
            "status": self.status,
            "headers": self.headers,
            "body": body,
        }))
    }
}

pub(crate) fn validate_http_url(
    url: &reqwest::Url,
    policy: &HostPolicy,
) -> Result<String, HttpErrorValue> {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(http_error(
            "R_HTTP_URL",
            "only HTTP and HTTPS URLs without embedded credentials are allowed",
        ));
    }
    let host = url
        .host_str()
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
        .ok_or_else(|| http_error("R_HTTP_URL", "URL has no host"))?;
    if policy.allowed_http_hosts.is_empty() || !policy.allowed_http_hosts.contains(&host) {
        return Err(http_error(
            "R_NETWORK_DENIED",
            "network access to this host is not granted",
        ));
    }
    Ok(host)
}

pub(crate) fn request_headers(
    headers: &BTreeMap<String, String>,
    policy: &HostPolicy,
) -> Result<reqwest::header::HeaderMap, HttpErrorValue> {
    let mut result = reqwest::header::HeaderMap::new();
    let mut header_bytes = 0usize;
    for (name, value) in headers {
        header_bytes = header_bytes
            .saturating_add(name.len())
            .saturating_add(value.len());
        if header_bytes > policy.max_http_request_bytes {
            return Err(http_error(
                "R_HTTP_LIMIT",
                "HTTP request headers exceed the configured byte limit",
            ));
        }
        let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| http_error("R_HTTP_HEADER", "request header name is invalid"))?;
        if matches!(
            name.as_str(),
            "host" | "content-length" | "connection" | "transfer-encoding"
        ) {
            return Err(http_error(
                "R_HTTP_HEADER",
                "request header is managed by the HTTP client",
            ));
        }
        let value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| http_error("R_HTTP_HEADER", "request header value is invalid"))?;
        result.insert(name, value);
    }
    Ok(result)
}

async fn execute_http_bytes(
    client: &reqwest::Client,
    policy: &HostPolicy,
    method: &str,
    input: &str,
    headers: &BTreeMap<String, String>,
    body: &[u8],
    deadline: Instant,
) -> Result<HttpBytesResponse, HttpErrorValue> {
    if body.len() > policy.max_http_request_bytes {
        return Err(http_error(
            "R_HTTP_LIMIT",
            "HTTP request body exceeds the configured byte limit",
        ));
    }
    let mut method = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|_| http_error("R_HTTP_METHOD", "HTTP method is invalid"))?;
    if !matches!(
        method.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
    ) {
        return Err(http_error("R_HTTP_METHOD", "HTTP method is not supported"));
    }
    let mut url =
        reqwest::Url::parse(input).map_err(|_| http_error("R_HTTP_URL", "URL is invalid"))?;
    let initial_host = validate_http_url(&url, policy)?;
    let initial_origin = url.origin().ascii_serialization();
    let mut headers = request_headers(headers, policy)?;
    let mut body = body.to_vec();

    for redirect_count in 0..=policy.max_http_redirects {
        let host = validate_http_url(&url, policy)?;
        if host != initial_host || url.origin().ascii_serialization() != initial_origin {
            return Err(http_error(
                "R_HTTP_REDIRECT",
                "redirect changed origin; credentials were not forwarded",
            ));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(http_error("R_HTTP_TIMEOUT", "HTTP request timed out"));
        }
        let response = client
            .request(method.clone(), url.clone())
            .headers(headers.clone())
            .timeout(remaining)
            .body(body.clone())
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    http_error("R_HTTP_TIMEOUT", "HTTP request timed out")
                } else {
                    http_error("R_HTTP_REQUEST", "HTTP request failed")
                }
            })?;

        let status = response.status();
        if matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
            if let Some(location) = response.headers().get(reqwest::header::LOCATION) {
                if redirect_count == policy.max_http_redirects {
                    return Err(http_error(
                        "R_HTTP_REDIRECT",
                        "HTTP redirect limit exceeded",
                    ));
                }
                let location = location.to_str().map_err(|_| {
                    http_error("R_HTTP_REDIRECT", "HTTP redirect location is invalid")
                })?;
                let next_url = url
                    .join(location)
                    .map_err(|_| http_error("R_HTTP_REDIRECT", "HTTP redirect URL is invalid"))?;
                let next_host = validate_http_url(&next_url, policy)
                    .map_err(|_| http_error("R_HTTP_REDIRECT", "redirect host is not granted"))?;
                if next_host != initial_host
                    || next_url.origin().ascii_serialization() != initial_origin
                {
                    return Err(http_error(
                        "R_HTTP_REDIRECT",
                        "redirect changed origin; credentials were not forwarded",
                    ));
                }
                if (status.as_u16() == 303 && method != reqwest::Method::HEAD)
                    || (matches!(status.as_u16(), 301 | 302) && method == reqwest::Method::POST)
                {
                    method = reqwest::Method::GET;
                    body.clear();
                    for name in [
                        "content-type",
                        "content-encoding",
                        "content-language",
                        "content-location",
                    ] {
                        headers.remove(name);
                    }
                }
                url = next_url;
                continue;
            }
        }

        if response
            .content_length()
            .is_some_and(|length| length > policy.max_http_response_bytes as u64)
        {
            return Err(http_error(
                "R_HTTP_LIMIT",
                "HTTP response exceeds the configured byte limit",
            ));
        }
        let response_headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_ascii_lowercase(), value.to_owned()))
            })
            .fold(
                BTreeMap::<String, String>::new(),
                |mut result, (name, value)| {
                    result
                        .entry(name)
                        .and_modify(|existing| {
                            existing.push_str(", ");
                            existing.push_str(&value);
                        })
                        .or_insert(value);
                    result
                },
            );
        let mut response = response;
        let reserve = response
            .content_length()
            .unwrap_or(0)
            .min(policy.max_http_response_bytes as u64)
            .min(64 * 1024) as usize;
        let mut bytes = Vec::with_capacity(reserve);
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            if error.is_timeout() {
                http_error("R_HTTP_TIMEOUT", "HTTP request timed out")
            } else {
                http_error("R_HTTP_REQUEST", "could not read HTTP response")
            }
        })? {
            if bytes.len().saturating_add(chunk.len()) > policy.max_http_response_bytes {
                return Err(http_error(
                    "R_HTTP_LIMIT",
                    "HTTP response exceeds the configured byte limit",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(HttpBytesResponse {
            status: status.as_u16(),
            headers: response_headers,
            body: bytes,
        });
    }
    Err(http_error(
        "R_HTTP_REDIRECT",
        "HTTP redirect limit exceeded",
    ))
}

pub(crate) struct HttpRequestBytes<'a> {
    pub(crate) method: &'a str,
    pub(crate) input: &'a str,
    pub(crate) headers: &'a BTreeMap<String, String>,
    pub(crate) body: &'a [u8],
    pub(crate) timeout: Duration,
}

pub(crate) fn perform_http_request_bytes(
    host: &HostHttpClient,
    policy: &HostPolicy,
    request: HttpRequestBytes<'_>,
    budget: &Budget,
) -> Result<HttpBytesResponse, HttpErrorValue> {
    let HttpRequestBytes {
        method,
        input,
        headers,
        body,
        timeout: request_timeout,
    } = request;
    let deadline = Instant::now().checked_add(request_timeout).ok_or_else(|| {
        http_error(
            "R_HTTP_TIMEOUT",
            "HTTP timeout is outside the supported range",
        )
    })?;
    host.executor.block_on(async {
        let request = execute_http_bytes(
            &host.client,
            policy,
            method,
            input,
            headers,
            body,
            deadline,
        );
        tokio::pin!(request);
        let cancellation = async {
            loop {
                if budget.is_cancelled() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        tokio::pin!(cancellation);
        tokio::select! {
            biased;
            _ = &mut cancellation => Err(http_error("R_HTTP_CANCELLED", "HTTP request was cancelled")),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                Err(http_error("R_HTTP_TIMEOUT", "HTTP request timed out"))
            },
            result = &mut request => result,
        }
    })
}

pub(crate) fn perform_http_get(
    host: &HostHttpClient,
    policy: &HostPolicy,
    input: &str,
    budget: &Budget,
) -> Result<String, HttpErrorValue> {
    let response = perform_http_request_bytes(
        host,
        policy,
        HttpRequestBytes {
            method: "GET",
            input,
            headers: &BTreeMap::new(),
            body: &[],
            timeout: policy.http_timeout,
        },
        budget,
    )?;
    if !(200..300).contains(&response.status) {
        return Err(http_error(
            "R_HTTP_STATUS",
            format!("HTTP server returned status {}", response.status),
        ));
    }
    if response.body.len() > policy.max_text_bytes {
        return Err(http_error(
            "R_HTTP_LIMIT",
            "HTTP response exceeds the configured text byte limit",
        ));
    }
    String::from_utf8(response.body)
        .map_err(|_| http_error("R_HTTP_ENCODING", "HTTP response body is not valid UTF-8"))
}
