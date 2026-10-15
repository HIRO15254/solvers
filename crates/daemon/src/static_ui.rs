//! Optional public SPA assets; API authentication remains in the HTTP router.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use protocol::{ErrorCode, ErrorResponse};
use tiny_http::{Header, Request, Response};

use crate::http::decode_component;

pub struct StaticUi {
    root: PathBuf,
}

struct Asset {
    path: PathBuf,
    content_type: &'static str,
    no_cache: bool,
}

impl StaticUi {
    pub fn new(directory: &Path) -> Result<Self> {
        let root = directory
            .canonicalize()
            .with_context(|| format!("opening UI directory {}", directory.display()))?;
        anyhow::ensure!(
            root.is_dir(),
            "UI path is not a directory: {}",
            root.display()
        );
        Ok(Self { root })
    }

    fn resolve(&self, url_path: &str) -> std::result::Result<Asset, ErrorResponse> {
        let decoded = decode_component(url_path, false)?;
        let relative = decoded.strip_prefix('/').ok_or_else(invalid_path)?;
        if relative.starts_with('/')
            || relative.contains('\\')
            || relative.contains(':')
            || relative.contains('\0')
            || relative.split('/').any(|part| part == ".." || part == ".")
        {
            return Err(invalid_path());
        }
        let candidate = self.root.join(if relative.is_empty() {
            "index.html"
        } else {
            relative
        });
        match candidate.canonicalize() {
            Ok(path) if !path.starts_with(&self.root) => return Err(invalid_path()),
            Ok(path) if path.is_file() => {
                return Ok(Asset {
                    path,
                    content_type: content_type(&candidate),
                    no_cache: candidate
                        .file_name()
                        .is_some_and(|name| name == "index.html"),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(ErrorResponse::new(ErrorCode::Internal, error.to_string())),
        }
        if Path::new(relative).extension().is_none() {
            let index = self
                .root
                .join("index.html")
                .canonicalize()
                .map_err(|_| not_found())?;
            if !index.starts_with(&self.root) {
                return Err(invalid_path());
            }
            if index.is_file() {
                return Ok(Asset {
                    path: index,
                    content_type: "text/html; charset=utf-8",
                    no_cache: true,
                });
            }
        }
        Err(not_found())
    }

    pub fn serve(&self, request: Request, path: &str) -> Result<()> {
        let asset = match self.resolve(path) {
            Ok(asset) => asset,
            Err(error) => return crate::http::respond_error(request, error),
        };
        let file = std::fs::File::open(&asset.path)
            .with_context(|| format!("opening UI asset {}", asset.path.display()))?;
        let mut response = Response::from_file(file).with_header(
            Header::from_bytes("Content-Type", asset.content_type)
                .map_err(|()| anyhow::anyhow!("building the content-type header"))?,
        );
        if asset.no_cache {
            response.add_header(
                Header::from_bytes("Cache-Control", "no-cache")
                    .map_err(|()| anyhow::anyhow!("building the cache-control header"))?,
            );
        }
        request.respond(response).context("writing the UI response")
    }
}

fn invalid_path() -> ErrorResponse {
    ErrorResponse::new(ErrorCode::InvalidRequest, "invalid UI asset path")
}

fn not_found() -> ErrorResponse {
    ErrorResponse::new(ErrorCode::NotFound, "UI asset not found")
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("json" | "map") => "application/json",
        Some("woff2") => "font/woff2",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_traversal_and_absolute_components() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join("index.html"), "index").unwrap();
        let ui = StaticUi::new(home.path()).unwrap();
        for path in [
            "/../secret",
            "/%2e%2e/secret",
            "/folder/../../secret",
            "//absolute",
            "/%2fabsolute",
            "/C:/secret",
            "/C%3a/secret",
            "/folder%5csecret",
            "relative",
            "/./index.html",
            "/%00",
        ] {
            assert!(ui.resolve(path).is_err(), "accepted {path}");
        }
    }

    #[test]
    fn missing_extensionless_routes_fall_back_but_assets_do_not() {
        let home = tempfile::tempdir().unwrap();
        let index = home.path().join("index.html");
        std::fs::write(&index, "index").unwrap();
        let ui = StaticUi::new(home.path()).unwrap();
        let root = ui.resolve("/").unwrap();
        assert_eq!(root.path, index.canonicalize().unwrap());
        assert!(root.no_cache);
        assert_eq!(root.content_type, "text/html; charset=utf-8");
        assert_eq!(
            ui.resolve("/runs/one").unwrap().path,
            index.canonicalize().unwrap()
        );
        assert!(ui.resolve("/missing.js").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_outside_ui_directory() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(outside.path(), home.path().join("outside")).unwrap();
        let ui = StaticUi::new(home.path()).unwrap();
        assert!(ui.resolve("/outside/secret.txt").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn index_symlink_retains_html_type_and_cache_policy() {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join("index-content"), "index").unwrap();
        std::os::unix::fs::symlink("index-content", home.path().join("index.html")).unwrap();
        let ui = StaticUi::new(home.path()).unwrap();
        for path in ["/", "/index.html", "/runs/example"] {
            let asset = ui.resolve(path).unwrap();
            assert!(asset.no_cache);
            assert_eq!(asset.content_type, "text/html; charset=utf-8");
        }
    }
}
