# rust-https

Minimal HTTP/HTTPS server library using raw TCP sockets. No async. Each connection gets its own thread.

## Usage

```rust
use rust_https::Server;

// HTTP
let server = Server::new_http("0.0.0.0:8080").unwrap();
server.serve(|body: &[u8]| {
    format!("received {} bytes", body.len()).into_bytes()
});
```

The handler receives the request body bytes and returns response body bytes. The library handles HTTP framing.

## Examples

```sh
# hello world
cargo run --example hello_world

# test with curl
curl http://localhost:8080
curl -d "hello" http://localhost:8080
```

## HTTPS

Requires the `tls` feature (enabled by default).

```rust
let server = Server::new_https("0.0.0.0:443", "cert.pem", "key.pem").unwrap();
```

Generate a self-signed cert for testing:

```sh
openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -nodes \
  -subj "/CN=localhost" \
  -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" \
  -addext "basicConstraints=critical,CA:FALSE" \
  -addext "keyUsage=critical,digitalSignature,keyEncipherment" \
  -addext "extendedKeyUsage=serverAuth"
```

## Tests

```sh
cargo test
```

Without TLS:

```sh
cargo build --no-default-features
```
