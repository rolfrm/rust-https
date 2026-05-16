use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

pub struct Request<'a> {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body_head: Vec<u8>,
    stream: &'a mut dyn Read,
}

impl std::fmt::Debug for Request<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("headers", &self.headers)
            .field("body_head", &self.body_head)
            .field("stream", &"<stream>")
            .finish()
    }
}

impl<'a> Request<'a> {
    pub fn get_header<T: std::str::FromStr>(&self, name: &str) -> Option<T> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .and_then(|(_, v)| v.parse().ok())
    }

    pub fn response(&self, status: u16, body: impl AsRef<[u8]>, content_type: &str) -> Vec<u8> {
        let body = body.as_ref();
        let reason = status_reason(status);
        format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n",
            body.len(),
        )
        .into_bytes()
        .into_iter()
        .chain(body.iter().copied())
        .collect()
    }

    pub fn read_body(&mut self) -> std::io::Result<Vec<u8>> {
        let content_length: usize = self.get_header("content-length").unwrap_or(0);
        let mut body = std::mem::take(&mut self.body_head);
        if content_length > body.len() {
            let mut rest = vec![0u8; content_length - body.len()];
            self.stream.read_exact(&mut rest)?;
            body.extend(rest);
        }
        Ok(body)
    }

    pub fn copy_body_to_stream<T: ?Sized>(&mut self, write_stream : &mut T) where T: Write{
        std::io::copy(self.stream, write_stream);
    }
}

fn status_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        418 => "I'm a Teapot",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "",
    }
}

pub type Handler = Arc<dyn for<'a> Fn(Request<'a>) -> Vec<u8> + Send + Sync>;

pub enum Server {
    Http(TcpListener),
    #[cfg(feature = "tls")]
    Https(TcpListener, Arc<rustls::ServerConfig>),
}

impl Server {
    pub fn new_http(addr: &str) -> std::io::Result<Self> {
        Ok(Server::Http(TcpListener::bind(addr)?))
    }

    #[cfg(feature = "tls")]
    pub fn new_https(addr: &str, cert_path: &str, key_path: &str) -> std::io::Result<Self> {
        let certs = load_certs(cert_path)?;
        let key = load_private_key(key_path)?;
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(Server::Https(TcpListener::bind(addr)?, Arc::new(config)))
    }

    pub fn serve<F>(self, handler: F)
    where
        F: for<'a> Fn(Request<'a>) -> Vec<u8> + Send + Sync + 'static,
    {
        let handler = Arc::new(handler);
        match self {
            Server::Http(listener) => {
                for stream in listener.incoming() {
                    match stream {
                        Ok(s) => {
                            thread::spawn({
                                let h = Arc::clone(&handler);
                                move || serve_http(s, h)
                            });
                        }
                        Err(e) => eprintln!("accept: {e}"),
                    }
                }
            }
            #[cfg(feature = "tls")]
            Server::Https(listener, tls) => {
                for stream in listener.incoming() {
                    match stream {
                        Ok(s) => {
                            thread::spawn({
                                let h = Arc::clone(&handler);
                                let t = Arc::clone(&tls);
                                move || serve_https(s, h, t)
                            });
                        }
                        Err(e) => eprintln!("accept: {e}"),
                    }
                }
            }
        }
    }
}

fn serve_http(stream: TcpStream, handler: Handler) {
    if let Err(e) = handle(stream, handler) {
        eprintln!("http: {e}");
    }
}

#[cfg(feature = "tls")]
#[allow(deprecated)]
fn serve_https(stream: TcpStream, handler: Handler, config: Arc<rustls::ServerConfig>) {
    let mut conn = match rustls::ServerConnection::new(config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("tls: {e}");
            return;
        }
    };
    let mut tcp = stream;
    let mut tls = rustls::Stream::new(&mut conn, &mut tcp);
    if let Err(e) = handle(&mut tls, handler) {
        eprintln!("https: {e}");
    }
}

pub(crate) fn handle(mut stream: impl Read + Write, handler: Handler) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let mut header_end = None;

    while header_end.is_none() {
        let n = stream.read(&mut tmp)?;
        if n == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof"));
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            header_end = Some(pos + 4);
        }
    }

    let header_end = header_end.unwrap();
    let extra = &buf[header_end..];
    let header_bytes = &buf[..header_end.saturating_sub(4)];

    let req = parse_request(header_bytes, extra, &mut stream);
    let res = handler(req);
    stream.write_all(&res)?;
    Ok(())
}

fn parse_request<'a>(header_bytes: &[u8], extra: &[u8], stream: &'a mut dyn Read) -> Request<'a> {
    let header_str = std::str::from_utf8(header_bytes).unwrap_or("");
    let mut lines = header_str.lines();

    let request_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    let method = parts.first().unwrap_or(&"").to_string();
    let path = parts.get(1).unwrap_or(&"").to_string();

    let mut headers = Vec::new();
    for line in lines {
        if let Some(pos) = line.find(':') {
            let key = line[..pos].trim().to_string();
            let value = line[pos + 1..].trim().to_string();
            headers.push((key, value));
        }
    }

    Request {
        method,
        path,
        headers,
        body_head: extra.to_vec(),
        stream,
    }
}

#[cfg(feature = "tls")]
fn load_certs(path: &str) -> std::io::Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    use std::io::BufReader;
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(certs)
}

#[cfg(feature = "tls")]
fn load_private_key(path: &str) -> std::io::Result<rustls::pki_types::PrivateKeyDer<'static>> {
    use std::io::BufReader;
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::new(file);
    let key = rustls_pemfile::private_key(&mut reader)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "no private key found"))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn get_empty_body() {
        let req = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let mut cur = Cursor::new(req.to_vec());
        let handler: Handler = Arc::new(|r: Request| {
            assert_eq!(r.method, "GET");
            assert_eq!(r.path, "/");
            assert_eq!(r.headers[0], ("Host".into(), "localhost".into()));
            assert!(r.body_head.is_empty());
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_vec()
        });
        handle(&mut cur, handler).unwrap();
        let resp = String::from_utf8(cur.into_inner()).unwrap();
        assert!(resp.ends_with("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"));
    }

    #[test]
    fn post_with_body() {
        let req = b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\n\r\nhello";
        let mut cur = Cursor::new(req.to_vec());
        let handler: Handler = Arc::new(|mut r: Request| {
            assert_eq!(r.method, "POST");
            let body = r.read_body().unwrap();
            assert_eq!(body, b"hello");
            b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nreceived".to_vec()
        });
        handle(&mut cur, handler).unwrap();
        let resp = String::from_utf8(cur.into_inner()).unwrap();
        assert!(resp.ends_with("HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nreceived"));
    }

    #[test]
    fn handler_output_is_response() {
        let req = b"GET / HTTP/1.1\r\n\r\n";
        let mut cur = Cursor::new(req.to_vec());
        let handler: Handler = Arc::new(|_: Request| {
            b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\nHello, World!".to_vec()
        });
        handle(&mut cur, handler).unwrap();
        let resp = String::from_utf8(cur.into_inner()).unwrap();
        assert!(resp.ends_with("Hello, World!"));
    }

    #[test]
    fn no_content_length_returns_empty_body() {
        let req = b"GET / HTTP/1.1\r\n\r\n";
        let mut cur = Cursor::new(req.to_vec());
        let handler: Handler = Arc::new(|mut r: Request| {
            let body = r.read_body().unwrap();
            assert!(body.is_empty());
            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_vec()
        });
        handle(&mut cur, handler).unwrap();
    }

    #[test]
    fn body_split_across_reads() {
        let header = b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\n";
        let body = b"hello";
        let mut full = header.to_vec();
        full.extend_from_slice(body);
        let mut cur = Cursor::new(full);
        let handler: Handler = Arc::new(|mut r: Request| {
            let buf = r.read_body().unwrap();
            assert_eq!(buf, b"hello");
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_vec()
        });
        handle(&mut cur, handler).unwrap();
    }
}
