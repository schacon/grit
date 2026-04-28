# GitHub CORS Proxy

This is a small Vercel-deployable CORS proxy for Git smart HTTP traffic. It lets
the browser/WASM client talk to a Git host that does not emit browser CORS
headers itself.

The proxy is intentionally not an open proxy. By default it only forwards to:

- `github.com`
- `www.github.com`
- `gist.github.com`

## Deploy

```bash
npm install
npm run deploy:prod
```

Or run locally:

```bash
npm install
npm run dev
```

## URL Shape

Use the deployed proxy as the repository base URL:

```text
https://<your-vercel-app>.vercel.app/OWNER/REPO.git
```

The WASM Git client will append normal smart HTTP paths:

```text
/info/refs?service=git-upload-pack
/git-upload-pack
/info/refs?service=git-receive-pack
/git-receive-pack
```

For example, in the demo page enter:

```text
https://<your-vercel-app>.vercel.app/rust-lang/rust.git
```

With the custom domain discussed for this project:

```text
https://github-proxy.io/rust-lang/rust.git
```

The original explicit API form still works too:

```text
https://<your-vercel-app>.vercel.app/api/git/github.com/OWNER/REPO.git
```

## Authentication

Prefer sending credentials as an `Authorization` header to the proxy. The proxy
forwards `Authorization` to GitHub.

For private GitHub repositories, use a token with the minimal scopes needed for
the operation. Avoid logging request headers in production.

## Security Notes

Before using this publicly:

- Keep the host allowlist restrictive.
- Add authentication or rate limiting for your proxy.
- Consider restricting repository owners/names.
- Be aware of Vercel function time and response size limits. Blobless fetches
  help, but large repos or pushes can still exceed serverless limits.

## CORS Headers

The proxy answers preflight requests and emits:

```http
Access-Control-Allow-Origin: *
Access-Control-Allow-Methods: GET, POST, OPTIONS
Access-Control-Allow-Headers: Authorization, Content-Type, Git-Protocol, Accept
Access-Control-Expose-Headers: Content-Type, Git-Protocol, WWW-Authenticate
```

Set `ALLOW_ORIGIN=https://your-app.example` in Vercel if you want to restrict
the allowed origin instead of using `*`.
