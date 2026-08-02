#!/usr/bin/env node
/**
 * Static preview of the Horizon Map webview assets.
 *
 * Serves `media/` with the same `/static/*` layout the desktop viewer uses,
 * and optionally proxies `/api/*` to a locally running horizon-server.
 *
 *   # terminal 1 — start the sidecar (any port)
 *   cargo run -p horizon-server -- --no-open
 *
 *   # terminal 2 — preview the extension media against it
 *   HORIZON_SIDECAR_URL=http://127.0.0.1:PORT npm run preview
 *
 * Open http://127.0.0.1:5179/ — analyse uses HTTP (non-IDE path).
 */

"use strict";

const http = require("http");
const fs = require("fs");
const path = require("path");
const { URL } = require("url");

const MEDIA = path.join(__dirname, "..", "media");
const PORT = Number(process.env.HORIZON_MAP_PREVIEW_PORT || 5179);
const SIDECAR = (process.env.HORIZON_SIDECAR_URL || "").replace(/\/?$/, "");

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
};

function sendFile(res, filePath) {
  const ext = path.extname(filePath);
  const type = MIME[ext] || "application/octet-stream";
  fs.readFile(filePath, (err, data) => {
    if (err) {
      res.writeHead(404, { "Content-Type": "text/plain" });
      res.end("not found");
      return;
    }
    res.writeHead(200, { "Content-Type": type });
    res.end(data);
  });
}

function proxyApi(req, res, apiPath) {
  if (!SIDECAR) {
    res.writeHead(503, { "Content-Type": "application/json" });
    res.end(
      JSON.stringify({
        error:
          "Set HORIZON_SIDECAR_URL=http://127.0.0.1:PORT to proxy /api/*",
      })
    );
    return;
  }

  let target;
  try {
    target = new URL(apiPath, SIDECAR + "/");
  } catch {
    res.writeHead(500, { "Content-Type": "text/plain" });
    res.end("bad sidecar url");
    return;
  }

  if (!/^https?:\/\/(127\.0\.0\.1|localhost|\[::1\])(:\d+)?$/i.test(target.origin)) {
    res.writeHead(403, { "Content-Type": "text/plain" });
    res.end("refusing non-localhost sidecar");
    return;
  }

  const chunks = [];
  req.on("data", (c) => chunks.push(c));
  req.on("end", () => {
    const body = Buffer.concat(chunks);
    const headers = {
      Accept: req.headers.accept || "application/json",
      "Content-Type": req.headers["content-type"] || "application/json",
      Host: target.host,
    };
    const upstream = http.request(
      {
        protocol: target.protocol,
        hostname: target.hostname,
        port: target.port,
        path: target.pathname + target.search,
        method: req.method,
        headers,
      },
      (up) => {
        res.writeHead(up.statusCode || 502, {
          "Content-Type": up.headers["content-type"] || "application/json",
        });
        up.pipe(res);
      }
    );
    upstream.on("error", (err) => {
      res.writeHead(502, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ error: String(err.message || err) }));
    });
    if (body.length) upstream.write(body);
    upstream.end();
  });
}

const server = http.createServer((req, res) => {
  const u = new URL(req.url || "/", `http://127.0.0.1:${PORT}`);
  const p = u.pathname;

  if (p.startsWith("/api/")) {
    proxyApi(req, res, p + u.search);
    return;
  }

  if (p === "/" || p === "/index.html") {
    sendFile(res, path.join(MEDIA, "index.html"));
    return;
  }

  if (p.startsWith("/static/")) {
    const rel = p.slice("/static/".length);
    const file = path.normalize(path.join(MEDIA, rel));
    if (!file.startsWith(MEDIA)) {
      res.writeHead(403).end("forbidden");
      return;
    }
    sendFile(res, file);
    return;
  }

  // Convenience: allow /viewer.js etc. without /static prefix
  const direct = path.normalize(path.join(MEDIA, p.replace(/^\//, "")));
  if (direct.startsWith(MEDIA) && fs.existsSync(direct) && fs.statSync(direct).isFile()) {
    sendFile(res, direct);
    return;
  }

  res.writeHead(404, { "Content-Type": "text/plain" });
  res.end("not found");
});

server.listen(PORT, "127.0.0.1", () => {
  console.log(`Horizon Map preview  http://127.0.0.1:${PORT}/`);
  if (SIDECAR) {
    console.log(`Proxying /api/* → ${SIDECAR}`);
  } else {
    console.log(
      "No HORIZON_SIDECAR_URL — UI only; set it to enable analyse/map/source."
    );
  }
});
