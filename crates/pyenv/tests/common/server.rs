//! A tiny HTTP/1.1 server on 127.0.0.1 for installer tests (spec §12.5 tier 1): each path
//! has a queue of replies, and the last one repeats. No network is used.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub enum Reply {
    /// 200 with this body (HEAD gets the headers only).
    Body(Vec<u8>),
    /// This status with an empty body.
    Status(u16),
    /// 200 announcing the full length but sending only the first half, then closing.
    Truncated(Vec<u8>),
}

pub struct Server {
    port: u16,
    hits: Arc<Mutex<HashMap<String, usize>>>,
}

impl Server {
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    pub fn hits(&self, path: &str) -> usize {
        *self.hits.lock().unwrap().get(path).unwrap_or(&0)
    }
}

pub fn start(routes: Vec<(&str, Vec<Reply>)>) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let queues: Arc<Mutex<HashMap<String, Vec<Reply>>>> = Arc::new(Mutex::new(
        routes
            .into_iter()
            .map(|(p, r)| (p.to_string(), r))
            .collect(),
    ));
    let hits = Arc::new(Mutex::new(HashMap::new()));
    let hits2 = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            if reader.read_line(&mut request).is_err() {
                continue;
            }
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }
            let mut parts = request.split_whitespace();
            let method = parts.next().unwrap_or("").to_string();
            let path = parts.next().unwrap_or("").to_string();
            *hits2.lock().unwrap().entry(path.clone()).or_insert(0) += 1;
            let reply = {
                let mut q = queues.lock().unwrap();
                match q.get_mut(&path) {
                    Some(v) if v.len() > 1 => v.remove(0),
                    Some(v) if v.len() == 1 => v[0].clone(),
                    _ => Reply::Status(404),
                }
            };
            let head = method == "HEAD";
            let _ = match reply {
                Reply::Body(b) => {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        b.len()
                    );
                    if head {
                        Ok(())
                    } else {
                        stream.write_all(&b)
                    }
                }
                Reply::Status(code) => write!(
                    stream,
                    "HTTP/1.1 {code} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                ),
                Reply::Truncated(b) => {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        b.len()
                    );
                    if head {
                        Ok(())
                    } else {
                        stream.write_all(&b[..b.len() / 2])
                    }
                }
            };
        }
    });
    Server { port, hits }
}
