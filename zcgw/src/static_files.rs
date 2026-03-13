use axum::{
    extract::Request,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web-ui/"]
struct WebUi;

pub async fn static_handler(req: Request) -> impl IntoResponse {
    let path = req.uri().path();

    // Strip /_app/ prefix for asset requests
    let file_path = if let Some(stripped) = path.strip_prefix("/_app/") {
        stripped.to_string()
    } else {
        // SPA fallback
        "index.html".to_string()
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
