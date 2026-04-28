import type { VercelRequest, VercelResponse } from "@vercel/node";

const ALLOWED_HOSTS = new Set([
  "github.com",
  "www.github.com",
  "gist.github.com",
]);

const HOP_BY_HOP_HEADERS = new Set([
  "connection",
  "keep-alive",
  "proxy-authenticate",
  "proxy-authorization",
  "te",
  "trailer",
  "transfer-encoding",
  "upgrade",
  "host",
]);

const FORWARDED_REQUEST_HEADERS = [
  "accept",
  "accept-encoding",
  "authorization",
  "content-type",
  "git-protocol",
  "user-agent",
];

function setCors(res: VercelResponse): void {
  res.setHeader("Access-Control-Allow-Origin", process.env.ALLOW_ORIGIN ?? "*");
  res.setHeader("Access-Control-Allow-Methods", "GET, POST, OPTIONS");
  res.setHeader(
    "Access-Control-Allow-Headers",
    "Authorization, Content-Type, Git-Protocol, Accept"
  );
  res.setHeader(
    "Access-Control-Expose-Headers",
    "Content-Type, Git-Protocol, WWW-Authenticate"
  );
  res.setHeader("Vary", "Origin");
}

function pathSegments(req: VercelRequest): string[] {
  const value = req.query.path;
  if (Array.isArray(value)) {
    return value.flatMap((part) => part.split("/")).filter(Boolean);
  }
  if (typeof value === "string") {
    return value.split("/").filter(Boolean);
  }
  return [];
}

function upstreamUrl(req: VercelRequest): URL {
  const segments = pathSegments(req);
  if (segments.length === 0) {
    throw new Error("missing repository path");
  }

  let normalizedHost = "github.com";
  let pathParts = segments;
  const first = segments[0].toLowerCase();
  if (ALLOWED_HOSTS.has(first)) {
    normalizedHost = first;
    pathParts = segments.slice(1);
  }

  const url = new URL(`https://${normalizedHost}/${pathParts.map(encodeURIComponent).join("/")}`);
  for (const [key, value] of Object.entries(req.query)) {
    if (key === "path") {
      continue;
    }
    if (Array.isArray(value)) {
      for (const item of value) {
        url.searchParams.append(key, item);
      }
    } else if (typeof value === "string") {
      url.searchParams.set(key, value);
    }
  }
  return url;
}

function requestHeaders(req: VercelRequest): Headers {
  const headers = new Headers();
  for (const name of FORWARDED_REQUEST_HEADERS) {
    const value = req.headers[name];
    if (Array.isArray(value)) {
      headers.set(name, value.join(", "));
    } else if (value) {
      headers.set(name, value);
    }
  }

  if (!headers.has("user-agent")) {
    headers.set("user-agent", "github-cors-proxy");
  }
  return headers;
}

function responseHeaders(upstream: Response): Record<string, string> {
  const headers: Record<string, string> = {};
  upstream.headers.forEach((value, key) => {
    const lower = key.toLowerCase();
    if (!HOP_BY_HOP_HEADERS.has(lower)) {
      headers[key] = value;
    }
  });
  return headers;
}

function requestBody(req: VercelRequest): BodyInit | undefined {
  if (req.method === "GET" || req.method === "HEAD") {
    return undefined;
  }
  if (Buffer.isBuffer(req.body)) {
    return new Uint8Array(req.body);
  }
  if (typeof req.body === "string") {
    return req.body;
  }
  if (req.body == null) {
    return undefined;
  }
  return JSON.stringify(req.body);
}

export default async function handler(req: VercelRequest, res: VercelResponse) {
  setCors(res);

  if (req.method === "OPTIONS") {
    res.status(204).end();
    return;
  }

  if (req.method !== "GET" && req.method !== "POST") {
    res.setHeader("Allow", "GET, POST, OPTIONS");
    res.status(405).send("method not allowed\n");
    return;
  }

  let url: URL;
  try {
    url = upstreamUrl(req);
  } catch (error) {
    res.status(400).send(`${error instanceof Error ? error.message : String(error)}\n`);
    return;
  }

  try {
    const upstream = await fetch(url, {
      method: req.method,
      headers: requestHeaders(req),
      body: requestBody(req),
      redirect: "manual",
    });

    for (const [key, value] of Object.entries(responseHeaders(upstream))) {
      res.setHeader(key, value);
    }
    setCors(res);
    res.status(upstream.status);

    const body = Buffer.from(await upstream.arrayBuffer());
    res.send(body);
  } catch (error) {
    res
      .status(502)
      .send(`upstream fetch failed: ${error instanceof Error ? error.message : String(error)}\n`);
  }
}
