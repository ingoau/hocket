/*
 * Hocket's YouTube Music JS host (docs/providers.md, Phase 4 "Premium JS
 * path"). Runs in a hidden browser page on https://www.youtube.com (a
 * WebView on Android, a BrowserWindow on desktop), after yt-dlp's EJS
 * solver (ejs-lib.min.js, ejs-core.min.js: `jsc`). The core asks through
 * `Event::YtmJsRequest`; the platform calls one of these and answers with
 * `Command::YtmJsAnswer`:
 *
 * - hocketYtm.prepare(videoId, visitorData) ->
 *     { signatureTimestamp, poToken }
 *   The current player's signature timestamp, and a PO token minted with
 *   BotGuard (as BgUtils / NewPipe's PoTokenWebView do), bound to the video
 *   id: a visitor-bound one gets googlevideo's 403 past the first MiB
 *   (checked live, Nov 2026). `poToken` is null when minting fails: the
 *   web client then only serves that first MiB.
 * - hocketYtm.decipher(videoId, formats) -> { urls }
 *   Each format is a plain URL (with an `n` parameter) or a
 *   `signatureCipher` query string; the answer is one playable URL each,
 *   signed and with `n` transformed by the player's own functions.
 */
(function () {
  "use strict";
  const ORIGIN = "https://www.youtube.com";
  const PLAYER_TTL_MS = 6 * 3600 * 1000;
  // BotGuard's web key and the request key the web player mints with.
  const BG_REQUEST_KEY = "O43z0dpjhgX20SCx4KAo";
  const BG_API_KEY = "AIzaSyDyT5W0Jh49F30Pqqtyfdf7pDLFKLJoAnw";

  let player = null; // { id, source, sts, prepared, at }
  let minter = null; // { mint, until }

  async function text(url, init) {
    const r = await fetch(url, init);
    if (!r.ok) throw new Error(`${url}: HTTP ${r.status}`);
    return r.text();
  }

  async function loadPlayer() {
    if (player && Date.now() - player.at < PLAYER_TTL_MS) return player;
    const api = await text(`${ORIGIN}/iframe_api`);
    const m = api.match(/player\\?\/([0-9a-zA-Z_-]+)\\?\//);
    if (!m) throw new Error("no player id in iframe_api");
    if (player && player.id === m[1]) {
      player.at = Date.now();
      return player;
    }
    const source = await text(`${ORIGIN}/s/player/${m[1]}/player_ias.vflset/en_US/base.js`);
    const sts = source.match(/(?:signatureTimestamp|sts)\s*:\s*(\d{5})/);
    player = { id: m[1], source, sts: sts ? Number(sts[1]) : null, prepared: null, at: Date.now() };
    return player;
  }

  /** Run the EJS solver over the player for these `{type, challenges}` requests. */
  function solve(p, requests) {
    const input = p.prepared
      ? { type: "preprocessed", preprocessed_player: p.prepared, requests }
      : { type: "player", player: p.source, requests, output_preprocessed: true };
    const out = jsc(input);
    if (out.preprocessed_player) p.prepared = out.preprocessed_player;
    if (out.type !== "result") throw new Error(out.error || "the solver failed");
    const by = {};
    requests.forEach((req, i) => {
      const r = out.responses[i];
      if (!r || r.type !== "result") throw new Error(`${req.type}: ${r ? r.error : "no answer"}`);
      by[req.type] = r.data;
    });
    return by;
  }

  /** `{url, s, sp}` from a plain URL or a `signatureCipher` query string. */
  function parseFormat(raw) {
    if (/^https?:/.test(raw)) return { url: raw, s: null, sp: null };
    const q = new URLSearchParams(raw);
    const url = q.get("url");
    if (!url) throw new Error("a format without a URL");
    return { url, s: q.get("s"), sp: q.get("sp") || "signature" };
  }

  async function decipher(_videoId, formats) {
    const parsed = formats.map(parseFormat);
    const sigs = parsed.map((f) => f.s).filter(Boolean);
    const ns = parsed.map((f) => new URL(f.url).searchParams.get("n")).filter(Boolean);
    const requests = [];
    if (sigs.length) requests.push({ type: "sig", challenges: [...new Set(sigs)] });
    if (ns.length) requests.push({ type: "n", challenges: [...new Set(ns)] });
    const solved = requests.length ? solve(await loadPlayer(), requests) : {};
    const urls = parsed.map((f) => {
      const u = new URL(f.url);
      if (f.s) u.searchParams.set(f.sp, solved.sig[f.s]);
      const n = u.searchParams.get("n");
      if (n) u.searchParams.set("n", solved.n[n]);
      return u.toString();
    });
    return { urls };
  }

  // -- PO token (BotGuard), after LuanRT/BgUtils ------------------------------

  async function jnn(method, body) {
    const r = await fetch(`${ORIGIN}/api/jnn/v1/${method}`, {
      method: "POST",
      headers: {
        "content-type": "application/json+protobuf",
        "x-goog-api-key": BG_API_KEY,
        "x-user-agent": "grpc-web-javascript/0.1",
      },
      body: JSON.stringify(body),
    });
    if (!r.ok) throw new Error(`BotGuard ${method}: HTTP ${r.status}`);
    return r.json();
  }

  function b64ToBytes(s) {
    const b = atob(s.replace(/-/g, "+").replace(/_/g, "/"));
    return Uint8Array.from(b, (c) => c.charCodeAt(0));
  }

  function bytesToB64Url(bytes) {
    let s = "";
    for (const b of bytes) s += String.fromCharCode(b);
    return btoa(s).replace(/\+/g, "-").replace(/\//g, "_");
  }

  async function challenge() {
    let data = await jnn("Create", [BG_REQUEST_KEY]);
    if (data.length > 1 && typeof data[1] === "string") {
      // Scrambled: base64, each byte shifted by 97.
      const bytes = b64ToBytes(data[1]);
      data = JSON.parse(String.fromCharCode(...bytes.map((b) => (b + 97) & 0xff)));
    }
    const [, wrappedScript, wrappedUrl, , program, globalName] = data;
    const pick = (w) => (Array.isArray(w) ? w.find((v) => v && typeof v === "string") : null);
    let script = pick(wrappedScript);
    if (!script) {
      const url = pick(wrappedUrl);
      if (!url) throw new Error("BotGuard: no interpreter");
      script = await text(url.startsWith("//") ? `https:${url}` : url);
    }
    return { script, program, globalName };
  }

  async function newMinter() {
    const c = await challenge();
    new Function(c.script)();
    const vm = globalThis[c.globalName];
    if (!vm || typeof vm.a !== "function") throw new Error("BotGuard: no VM");
    let snapshot = null;
    vm.a(c.program, (asyncSnapshot) => (snapshot = asyncSnapshot), true, undefined, () => {}, [[], []]);
    for (let i = 0; !snapshot && i < 50; i++) await new Promise((r) => setTimeout(r, 100));
    if (!snapshot) throw new Error("BotGuard: no snapshot function");
    const webPoSignalOutput = [];
    const response = await new Promise((resolve) =>
      snapshot((r) => resolve(r), [undefined, undefined, webPoSignalOutput, undefined]),
    );
    const it = await jnn("GenerateIT", [BG_REQUEST_KEY, response]);
    const [integrityToken, ttlSecs] = it;
    if (typeof integrityToken !== "string") throw new Error("BotGuard: no integrity token");
    const getMinter = webPoSignalOutput[0];
    if (typeof getMinter !== "function") throw new Error("BotGuard: no minter");
    const mint = await getMinter(b64ToBytes(integrityToken));
    const ttl = (typeof ttlSecs === "number" ? ttlSecs : 3600) * 1000;
    // Renew well before it runs out.
    return { mint, until: Date.now() + ttl * 0.8 };
  }

  async function poToken(identifier) {
    if (!minter || Date.now() > minter.until) minter = await newMinter();
    const bytes = await minter.mint(new TextEncoder().encode(identifier));
    if (!bytes || !bytes.length) throw new Error("BotGuard: empty token");
    return bytesToB64Url(bytes);
  }

  async function prepare(videoId, _visitorData) {
    const p = await loadPlayer();
    let token = null;
    try {
      token = await poToken(videoId);
    } catch (e) {
      minter = null;
      console.warn(`hocket: no PO token: ${e && e.message ? e.message : e}`);
    }
    return { signatureTimestamp: p.sts, poToken: token };
  }

  globalThis.hocketYtm = { prepare, decipher };
})();
