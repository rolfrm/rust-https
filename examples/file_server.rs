fn main() {
    let port = std::env::args().nth(1).unwrap_or_else(|| "8080".into());
    let addr = format!("0.0.0.0:{port}");
    let server = rust_https::Server::new_http(&addr).expect("failed to bind");
    println!("listening on http://{addr}");
    server.serve(|mut req: rust_https::Request| {
        let path = req.path.trim_start_matches('/').to_string();
        let path = if path.is_empty() { "index.html".into() } else { path };
        if path.contains("..") {
            let html = b"<html><body><h1>403 Forbidden</h1></body></html>";
            return req.response(403, &html[..], "text/html");
        }
        let ct = content_type(&path);
        match std::fs::metadata(&path) {
            Ok(_) => req.response_from_file(200, &path, ct),
            Err(_) => {
                let html = b"<html><body><h1>404 Not Found</h1></body></html>";
                req.response(404, &html[..], "text/html")
            }
        }
    });
}

fn content_type(path: &str) -> &'static str {
    if path.ends_with(".html") || path.ends_with(".htm") {
        "text/html"
    } else if path.ends_with(".css") {
        "text/css"
    } else if path.ends_with(".js") {
        "application/javascript"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "image/jpeg"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".txt") {
        "text/plain; charset=utf-8"
    } else if path.ends_with(".pdf") {
        "application/pdf"
    } else if path.ends_with(".ico") {
        "image/x-icon"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else {
        "application/octet-stream"
    }
}
