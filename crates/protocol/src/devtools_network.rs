//! DevTools' `Network` domain's objects as CDP gives them (`Network.Request`,
//! `Network.Response`, `Network.ResourceTiming`...), of the network
//! service's requests and responses: what the renderer's `Network` agent
//! and the browser's DevTools session tell alike.

use ipc::TimeTicks;
use serde_json::{Value, json};

use crate::network::{LoadTiming, RequestDestination, ResourceRequest, ResourceResponse, TimeSpan};

/// `Network.ResourceType` of a request for `destination`.
pub fn resource_type(destination: RequestDestination) -> &'static str {
    match destination {
        RequestDestination::Document | RequestDestination::Iframe => "Document",
        RequestDestination::Style => "Stylesheet",
        RequestDestination::Image => "Image",
        RequestDestination::Script => "Script",
        RequestDestination::Empty => "Fetch",
    }
}

/// Chrome's initial priority of a request of `kind`
/// (`ResourceFetcher::ComputeLoadPriority`, roughly).
pub fn priority(kind: &str) -> &'static str {
    match kind {
        "Document" | "Stylesheet" => "VeryHigh",
        "Image" => "Low",
        _ => "High",
    }
}

/// Seconds since the Unix epoch, now (DevTools' `TimeSinceEpoch`).
pub fn wall_time() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64())
}

/// Headers as DevTools' `Network.Headers`: an object, the values of a name
/// sent more than once joined with newlines (as Chrome's).
pub fn headers_json(headers: &[(String, String)]) -> Value {
    let mut object = serde_json::Map::new();
    for (name, value) in headers {
        match object.get_mut(name) {
            Some(Value::String(known)) => {
                known.push('\n');
                known.push_str(value);
            }
            _ => {
                object.insert(name.clone(), Value::String(value.clone()));
            }
        }
    }
    Value::Object(object)
}

/// `Network.Request` of `request`, its body as `postData` if no longer
/// than `max_post_data_size`.
pub fn request_json(request: &ResourceRequest, max_post_data_size: usize) -> Value {
    let (url, fragment) = match request.url.split_once('#') {
        Some((url, fragment)) => (url, Some(format!("#{fragment}"))),
        None => (request.url.as_str(), None),
    };
    let mut json = json!({
        "url": url,
        "method": request.method,
        "headers": headers_json(&request.headers),
        "initialPriority": priority(resource_type(request.destination)),
        "referrerPolicy": "strict-origin-when-cross-origin",
        "mixedContentType": "none",
    });
    if let Some(fragment) = fragment {
        json["urlFragment"] = fragment.into();
    }
    if let Some(body) = &request.body {
        json["hasPostData"] = true.into();
        if body.len() <= max_post_data_size {
            json["postData"] = String::from_utf8_lossy(body).into();
        }
    }
    json
}

/// `Network.Response` of `response`.
pub fn response_json(response: &ResourceResponse) -> Value {
    let mut json = json!({
        "url": response.url,
        "status": response.status,
        "statusText": response.status_text,
        "headers": headers_json(&response.headers),
        "mimeType": response.mime_type,
        "charset": response.charset.clone().unwrap_or_default(),
        "connectionReused": response.timing.is_some_and(|timing| timing.connect.is_none())
            && !response.protocol.is_empty()
            && response.remote_address.is_some(),
        "connectionId": 0,
        "fromDiskCache": false,
        "fromServiceWorker": false,
        "fromPrefetchCache": false,
        "encodedDataLength": response.encoded_headers_length,
        "protocol": response.protocol,
        "securityState": security_state(&response.url),
    });
    if let Some(address) = response.remote_address {
        json["remoteIPAddress"] = address.ip().to_string().into();
        json["remotePort"] = address.port().into();
    }
    if let Some(timing) = response.timing {
        json["timing"] = timing_json(&timing);
        json["responseTime"] = (wall_time() * 1000.0).into();
    }
    json
}

/// `Network.ResourceTiming` of `timing`: `requestTime` in seconds, the rest
/// milliseconds after it, -1 for what didn't happen.
pub fn timing_json(timing: &LoadTiming) -> Value {
    let start = timing.request_start;
    let ms = |at: TimeTicks| at.saturating_since(start).as_secs_f64() * 1000.0;
    let span =
        |span: Option<TimeSpan>| span.map_or((-1.0, -1.0), |span| (ms(span.start), ms(span.end)));
    let (dns_start, dns_end) = span(timing.dns);
    let (connect_start, connect_end) = span(timing.connect);
    // Sent once the connection is there.
    let send = timing.connect.map_or(0.0, |connect| ms(connect.end));
    let headers = ms(timing.receive_headers_end);
    json!({
        "requestTime": start.as_secs_f64(),
        "proxyStart": -1,
        "proxyEnd": -1,
        "dnsStart": dns_start,
        "dnsEnd": dns_end,
        "connectStart": connect_start,
        "connectEnd": connect_end,
        "sslStart": -1,
        "sslEnd": -1,
        "workerStart": -1,
        "workerReady": -1,
        "workerFetchStart": -1,
        "workerRespondWithSettled": -1,
        "sendStart": send,
        "sendEnd": send,
        "pushStart": 0,
        "pushEnd": 0,
        "receiveHeadersStart": headers,
        "receiveHeadersEnd": headers,
    })
}

/// `Security.SecurityState` of a response from `url`.
pub fn security_state(url: &str) -> &'static str {
    match url.split_once(':').map(|(scheme, _)| scheme) {
        Some("https" | "wss") => "secure",
        Some("http" | "ws") => "insecure",
        _ => "neutral",
    }
}
