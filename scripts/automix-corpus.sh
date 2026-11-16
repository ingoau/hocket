#!/usr/bin/env bash
# Build the AutoMix test corpus: ~100 varied tracks downloaded (original files,
# Subsonic `download`) from the test Navidrome server into the gitignored
# automix-corpus/, plus automix-corpus/tracks.json with their metadata.
#
#   scripts/automix-corpus.sh [count]        # default 100; re-runs skip files already there
#
# Credentials come from ~/.config/hocket/test-server.env (HOCKET_TEST_URL,
# HOCKET_TEST_USER, HOCKET_TEST_PASS) or the environment. They are never printed.
# Tracks: up to 3 per genre across the server's genres, two same-album runs
# (consecutive tracks, for the album-sequence case), the tracks named in
# crates/hocket-core/src/automix/corpus/pairs.txt, and random songs to fill.
# Then `cargo run -p hocket-mixlab --release -- stats automix-corpus`.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
env_file="${HOCKET_TEST_ENV:-$HOME/.config/hocket/test-server.env}"
if [[ -z "${HOCKET_TEST_URL:-}" && -f "$env_file" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "$env_file"
  set +a
fi
: "${HOCKET_TEST_URL:?set HOCKET_TEST_URL (or provide $env_file)}"
: "${HOCKET_TEST_USER:?}" "${HOCKET_TEST_PASS:?}"
export HOCKET_TEST_URL HOCKET_TEST_USER HOCKET_TEST_PASS

out="$root/automix-corpus"
mkdir -p "$out"
COUNT="${1:-100}" OUT="$out" PAIRS="$root/crates/hocket-core/src/automix/corpus/pairs.txt" \
  exec python3 -I - <<'PY'
import hashlib, json, os, random, re, secrets, sys, urllib.parse, urllib.request

base = os.environ["HOCKET_TEST_URL"].rstrip("/")
user, password = os.environ["HOCKET_TEST_USER"], os.environ["HOCKET_TEST_PASS"]
out, count = os.environ["OUT"], int(os.environ["COUNT"])
rng = random.Random(7)

def url(method, **params):
    salt = secrets.token_hex(6)
    q = dict(u=user, t=hashlib.md5((password + salt).encode()).hexdigest(), s=salt,
             v="1.16.1", c="hocket-corpus", f="json", **params)
    return f"{base}/rest/{method}?{urllib.parse.urlencode(q)}"

def fetch(u, timeout):
    req = urllib.request.Request(u, headers={"User-Agent": "hocket-corpus/1"})
    return urllib.request.urlopen(req, timeout=timeout)

def call(method, **params):
    with fetch(url(method, **params), 60) as r:
        body = json.load(r)["subsonic-response"]
    if body.get("status") != "ok":
        sys.exit(f"{method}: {body.get('error', {}).get('message', 'failed')}")
    return body

def as_list(x):
    return x if isinstance(x, list) else ([] if x is None else [x])

chosen = {}
def take(song, why):
    if song.get("isVideo") or song["id"] in chosen:
        return
    if song.get("duration", 0) < 60 or song.get("duration", 0) > 900:
        return
    chosen[song["id"]] = dict(song, why=why)

# Tracks the committed pair list needs.
pairs = os.environ["PAIRS"]
if os.path.exists(pairs):
    for line in open(pairs):
        line = line.split("#")[0].split()
        for tid in line[:2]:
            take(call("getSong", id=tid)["song"], "pairs")

genres = [g["value"] for g in as_list(call("getGenres").get("genres", {}).get("genre"))
          if g.get("songCount", 0) > 0]
rng.shuffle(genres)
for g in genres:
    if len(chosen) >= count * 2 // 3:
        break
    songs = as_list(call("getSongsByGenre", genre=g, count=50).get("songsByGenre", {}).get("song"))
    rng.shuffle(songs)
    for s in songs[:3]:
        take(s, f"genre:{g}")

# Two album runs: three consecutive tracks each.
albums = as_list(call("getAlbumList2", type="random", size=20).get("albumList2", {}).get("album"))
runs = 0
for a in albums:
    if runs == 2:
        break
    songs = sorted(as_list(call("getAlbum", id=a["id"])["album"].get("song")),
                   key=lambda s: (s.get("discNumber", 1), s.get("track", 0)))
    if len(songs) >= 4:
        for s in songs[1:4]:
            take(s, f"album:{a['id']}")
        runs += 1

while len(chosen) < count:
    before = len(chosen)
    for s in as_list(call("getRandomSongs", size=50).get("randomSongs", {}).get("song")):
        if len(chosen) < count:
            take(s, "random")
    if len(chosen) == before:
        break

keep = ("id", "title", "artist", "album", "albumId", "genre", "track", "discNumber",
        "duration", "suffix", "size", "bitRate", "year", "why")
def file_name(song):
    """The server's id and suffix, made safe for a path: anything but
    [A-Za-z0-9_-] in the id becomes `_` plus a hash of the id (so ids
    differing only there stay apart); the suffix keeps only alphanumerics."""
    sid = str(song["id"])
    safe = re.sub(r"[^A-Za-z0-9_-]", "_", sid)[:100]
    if safe != sid or not safe:
        safe = f"{safe}-{hashlib.sha256(sid.encode()).hexdigest()[:12]}"
    suffix = re.sub(r"[^A-Za-z0-9]", "", str(song.get("suffix") or ""))[:8] or "bin"
    return f"{safe}.{suffix}"

meta = []
for i, s in enumerate(chosen.values(), 1):
    path = os.path.join(out, file_name(s))
    if not os.path.exists(path):
        print(f"[{i}/{len(chosen)}] {s.get('artist', '?')} - {s.get('title', '?')}", flush=True)
        tmp = path + ".part"
        try:
            with fetch(url("download", id=s["id"]), 300) as r, open(tmp, "wb") as f:
                while chunk := r.read(1 << 20):
                    f.write(chunk)
            os.replace(tmp, path)
        except BaseException:
            if os.path.exists(tmp):
                os.remove(tmp)
            raise
    meta.append({k: s[k] for k in keep if k in s} | {"file": os.path.basename(path)})
with open(os.path.join(out, "tracks.json"), "w") as f:
    json.dump(meta, f, indent=1, ensure_ascii=False)
print(f"{len(meta)} tracks in {out}")
PY
