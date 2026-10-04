//! python.org as the tier-1 fake server presents it (plan M2b): nginx listings, Install
//! Manager index pages and small zips.

use std::io::Write;

/// An nginx autoindex page linking `names`.
pub fn listing(names: &[&str]) -> Vec<u8> {
    let mut s = String::from("<html><body><pre><a href=\"../\">../</a>\n");
    for n in names {
        s.push_str(&format!("<a href=\"{n}\">{n}</a>\n"));
    }
    s.push_str("</pre></body></html>\n");
    s.into_bytes()
}

/// An index page. `zips` are `(url, sha256)` for `python-<ver>[t]-<arch>.zip` URLs.
pub fn index_json(zips: &[(&str, &str)], next: Option<&str>) -> Vec<u8> {
    let rows: Vec<String> = zips
        .iter()
        .map(|(url, sha)| {
            let file = url.rsplit('/').next().unwrap();
            let ver = file.trim_start_matches("python-").rsplit_once('-').unwrap().0.trim_end_matches('t');
            format!("{{\"schema\":1,\"id\":\"pythoncore\",\"sort-version\":\"{ver}\",\"company\":\"PythonCore\",\"url\":\"{url}\",\"hash\":{{\"sha256\":\"{sha}\"}}}}")
        })
        .collect();
    let next = next
        .map(|n| format!(",\"next\":\"{n}\""))
        .unwrap_or_default();
    format!("{{\"versions\":[{}]{next}}}", rows.join(",")).into_bytes()
}

/// A deflated zip of `(name, body)` entries.
pub fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in entries {
        w.start_file(*name, opts).unwrap();
        w.write_all(body).unwrap();
    }
    w.finish().unwrap().into_inner()
}

pub fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
