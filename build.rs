use std::{env, fs, path::Path};

fn main() {
    let root = env::var("CARGO_MANIFEST_DIR").expect("Cargo package directory");
    let directory = Path::new(&root).join("web/dist");
    println!("cargo:rerun-if-changed=web/dist");
    assert!(
        directory.join("index.html").is_file(),
        "Web UIを先にビルドしてください: npm --prefix web ci && npm --prefix web run build"
    );
    let mut files = Vec::new();
    collect_files(&directory, &mut files);
    files.sort();
    let mut source = String::from("fn lookup(path: &str) -> Option<Asset> { match path {\n");
    for file in files {
        let relative = file.strip_prefix(&directory).unwrap().to_str().expect("UTF-8 asset path");
        let content_type = match file.extension().and_then(|extension| extension.to_str()) {
            Some("html") => "text/html; charset=utf-8",
            Some("js") => "text/javascript; charset=utf-8",
            Some("css") => "text/css; charset=utf-8",
            Some("svg") => "image/svg+xml",
            Some("png") => "image/png",
            Some("woff2") => "font/woff2",
            _ => "application/octet-stream",
        };
        source.push_str(&format!(
            "{path:?} => Some(Asset {{ content_type: {content_type:?}, bytes: include_bytes!({file:?}) }}),\n",
            path = format!("/{relative}")
        ));
    }
    source.push_str("_ => None, } }\n");
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("web_assets.rs"), source).expect("Write embedded assets");
}

fn collect_files(directory: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(directory).expect("Read Web UI assets") {
        let entry = entry.expect("Read asset entry");
        let kind = entry.file_type().expect("Read asset type");
        if kind.is_dir() {
            collect_files(&entry.path(), files);
        } else if kind.is_file() {
            files.push(entry.path());
        }
    }
}
