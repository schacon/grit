# Grit WASM Blobless Clone Demo

This example loads the generated `grit-wasm` package, accepts a smart HTTP Git
URL, performs a blobless clone, and shows the latest five commit messages.

## Build

From the repository root:

```bash
wasm-pack build grit-wasm --target web --out-dir examples/pkg
```

Then serve `grit-wasm/examples/github-log/` with any static file server.

```bash
python3 -m http.server --directory grit-wasm/examples/github-log 8080
```

Open <http://127.0.0.1:8080>.

## CORS

Browser smart HTTP requests require CORS. Public GitHub smart HTTP endpoints may
not allow direct browser fetches. For local testing, serve a repository with the
project test server:

```bash
cargo build -p grit-rs --bin test-httpd
target/debug/test-httpd --root /tmp/git-http-root --cors --port 0
```

Use a URL like:

```text
http://127.0.0.1:<port>/smart/repo.git
```
