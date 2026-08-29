use axum::{
    body::Body,
    extract::OriginalUri,
    http::{StatusCode, header},
    response::Response,
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/dist/"]
struct WebAssets;

pub async fn asset(OriginalUri(uri): OriginalUri) -> Response {
    let requested = uri.path().trim_start_matches('/');
    if requested.starts_with("api/") {
        return response(
            StatusCode::NOT_FOUND,
            "application/json; charset=utf-8",
            br#"{"error":{"code":"NotFound","message":"API route was not found"}}"#.to_vec(),
            "no-store",
        );
    }

    let path = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };
    if let Some(asset) = WebAssets::get(path) {
        return response(
            StatusCode::OK,
            content_type(path),
            asset.data.into_owned(),
            if path == "index.html" {
                "no-cache"
            } else {
                "public, max-age=31536000, immutable"
            },
        );
    }

    match WebAssets::get("index.html") {
        Some(index) => response(
            StatusCode::OK,
            "text/html; charset=utf-8",
            index.data.into_owned(),
            "no-cache",
        ),
        None => response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "text/plain; charset=utf-8",
            b"embedded web application is missing".to_vec(),
            "no-store",
        ),
    }
}

fn response(
    status: StatusCode,
    content_type: &'static str,
    body: Vec<u8>,
    cache_control: &'static str,
) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, cache_control)
        .header("X-Content-Type-Options", "nosniff")
        .body(Body::from(body))
        .expect("static response headers are valid")
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, extension)| extension) {
        Some("html") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_binary_contains_the_web_entrypoint() {
        let index = WebAssets::get("index.html").expect("web/dist must be built before Rust");
        assert!(String::from_utf8_lossy(&index.data).contains("<div id=\"root\"></div>"));
    }
}
