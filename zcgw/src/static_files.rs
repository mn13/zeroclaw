use axum::{
    extract::Request,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../web/dist/"]
struct WebUi;

pub async fn static_handler(req: Request) -> impl IntoResponse {
    let path = req.uri().path();

    // Strip leading slash and try to serve the file directly.
    // Handles /assets/*, /_app/*, and any other static file paths.
    let trimmed = path.strip_prefix('/').unwrap_or(path);
    let file_path = if trimmed.is_empty() {
        "index.html".to_string()
    } else {
        trimmed.to_string()
    };

    serve_file(&file_path)
}

fn serve_file(path: &str) -> Response {
    match WebUi::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                file.data.to_vec(),
            )
                .into_response()
        }
        None => {
            // Fallback to index.html for SPA routing
            match WebUi::get("index.html") {
                Some(file) => (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "text/html".to_string())],
                    file.data.to_vec(),
                )
                    .into_response(),
                None => (StatusCode::NOT_FOUND, "not found").into_response(),
            }
        }
    }
}
