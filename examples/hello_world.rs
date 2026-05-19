fn main() {
    let server = rust_https::Server::new_https("0.0.0.0:8080", "cert.pem", "key.pem").expect("failed to bind");
    println!("listening on http://0.0.0.0:8080");
    server.serve(|mut req: rust_https::Request| {
        let body = req.read_body().unwrap_or_default();
        let html = format!("<!doctype html><html><body><h1>Hello, World!</h1> {:?}</body></html>", req);
        req.response(200, html.as_bytes(), "text/html", &[])
    });
}
