#!/usr/bin/env python3
"""Runs the immersive-artwork classifier (crates/hocket-core/src/artwork) over a folder of covers
and draws what the phone's full player would look like for each, for tuning by eye.

    pip install pillow numpy opencv-python-headless
    cargo build -p hocket-core --example artwork_layout --release
    scripts/artwork-eval.py <covers dir> <out dir>

Writes <out>/layouts.jsonl (one per cover: file, faces, layout) and <out>/sheet-NN.jpg contact
sheets grouped by the style chosen. Faces come from OpenCV's frontal Haar cascade, standing in for
Android's FaceDetector (also frontal-only).
"""
import json, os, subprocess, sys
import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "examples", "artwork_layout")
W, H = 240, 520          # a phone, in "dp"
DECODE = 256             # what the platform hands the core (Android decodes downsampled)
FEATHER = 0.18           # share of the artwork that fades into the continuation
BLUR_NEAR = 6            # blur at the seam (px at W)
BLUR_FAR = 16            # blur far from it


def faces_of(img):
    """Faces as fractions of the image. With ARTWORK_EVAL_YUNET pointing at OpenCV Zoo's YuNet
    model (face_detection_yunet_2023mar.onnx) a modern detector; otherwise the frontal Haar
    cascade, roughly what Android's built-in FaceDetector finds."""
    import cv2
    yunet = os.environ.get("ARTWORK_EVAL_YUNET")
    if yunet:
        det = cv2.FaceDetectorYN.create(yunet, "", (320, 320), 0.7, 0.3, 50)
        bgr = np.array(img.convert("RGB").resize((320, 320)))[:, :, ::-1].copy()
        _, found = det.detect(bgr)
        return [] if found is None else [{"x": float(f[0]) / 320, "y": float(f[1]) / 320, "w": float(f[2]) / 320, "h": float(f[3]) / 320} for f in found]
    g = np.array(img.convert("L").resize((300, 300)))
    cascade = cv2.CascadeClassifier(cv2.data.haarcascades + "haarcascade_frontalface_default.xml")
    found = cascade.detectMultiScale(g, scaleFactor=1.1, minNeighbors=6, minSize=(18, 18))
    return [{"x": x / 300, "y": y / 300, "w": w / 300, "h": h / 300} for (x, y, w, h) in found]


def rgb(c):
    return ((c >> 16) & 255, (c >> 8) & 255, c & 255)


def ramp(h, stops):
    """A vertical alpha ramp (0..255) over h rows from (pos, alpha) stops."""
    ys = np.linspace(0, 1, h)
    xs, vs = zip(*stops)
    return np.interp(ys, xs, vs)


def mask(w, h, stops):
    return Image.fromarray(np.repeat(ramp(h, stops)[:, None], w, 1).astype(np.uint8), "L")


def render(art, layout, edge="bottom"):
    e = layout[edge]
    style = e["style"]
    base = rgb(e["baseColor"])
    phone = Image.new("RGB", (W, H), base)
    if style == "card":
        bg = art.resize((64, 64)).filter(ImageFilter.GaussianBlur(10)).resize((W, H))
        phone.paste(bg)
        phone = Image.blend(phone, Image.new("RGB", (W, H), (0, 0, 0)), 0.5)
        side = W - 32
        card = art.resize((side, side))
        m = Image.new("L", (side, side), 0)
        ImageDraw.Draw(m).rounded_rectangle((0, 0, side - 1, side - 1), 16, fill=255)
        phone.paste(card, (16, 56), m)
        fg = (255, 255, 255)
    else:
        top = art.resize((W, W))
        below = H - W
        # Far from the artwork every style ends on its blurred colours (the Card backdrop; moving
        # on the device); a flat extension stays its one colour all the way down.
        backdrop = art.resize((48, 48)).filter(ImageFilter.GaussianBlur(8)).resize((W, H)).crop((0, W, W, H))
        flat = style == "extend" and len(set(e["edgeColors"])) <= 1
        cont = Image.new("RGB", (W, below), base) if flat else backdrop
        # The seam: the artwork fades out over its last FEATHER of height onto a blurred copy of
        # itself, and the continuation starts at that same blur, so nothing cuts over at the edge.
        # A flat extension skips it: the artwork already ends in that colour, so it stays crisp.
        f = int(W * FEATHER)
        soft_art = top.filter(ImageFilter.GaussianBlur(BLUR_NEAR))
        if not flat:
            top.paste(soft_art.crop((0, W - f, W, W)), (0, W - f), mask(W, f, [(0, 0), (1, 255)]))
        if style == "mirror":
            flip = top.transpose(Image.FLIP_TOP_BOTTOM)
            sharp = Image.new("RGB", (W, below), base); sharp.paste(flip)
            near = sharp.filter(ImageFilter.GaussianBlur(BLUR_NEAR))
            far = sharp.filter(ImageFilter.GaussianBlur(BLUR_FAR))
            cont.paste(far, (0, 0), mask(W, below, [(0, 255), (0.5, 255), (0.9, 0)]))
            cont.paste(near, (0, 0), mask(W, below, [(0, 255), (0.12, 255), (0.4, 0)]))
        else:
            # The edge's colours carried down, as soft at the seam as the artwork's blurred edge,
            # softening further with distance, then fading into the backdrop.
            cols = [rgb(c) for c in e["edgeColors"]]
            row = Image.fromarray(np.array([cols], np.uint8)).resize((W, 1), Image.BILINEAR)
            wide = row.resize((W, 8)).filter(ImageFilter.GaussianBlur(28)).crop((0, 4, W, 5)).resize((W, below))
            soft = row.resize((W, 8)).filter(ImageFilter.GaussianBlur(BLUR_NEAR)).crop((0, 4, W, 5)).resize((W, below))
            cont.paste(wide, (0, 0), mask(W, below, [(0, 255), (0.5, 255), (0.9, 0)]))
            cont.paste(soft, (0, 0), mask(W, below, [(0, 255), (0.15, 255), (0.45, 0)]))
            # Bridge: a short, blurred reflection of the edge, so the colours take over from
            # exactly what the artwork's softened edge shows.
            bridge = Image.new("RGB", (W, below), base); bridge.paste(soft_art.transpose(Image.FLIP_TOP_BOTTOM))
            if not flat:
                cont.paste(bridge.filter(ImageFilter.GaussianBlur(BLUR_NEAR)), (0, 0), mask(W, below, [(0, 255), (0.1, 0)]))
        phone.paste(top, (0, 0))
        phone.paste(cont, (0, W))
        light = e["light"]
        fg = (28, 27, 31) if light else (255, 255, 255)
        scrim = (255, 255, 255) if light else (0, 0, 0)
        a = int(e["scrim"] * 255)
        if a:
            ov = Image.new("RGB", (W, H - W), scrim)
            phone.paste(ov, (0, W), mask(W, H - W, [(0, 0), (0.25, a), (1, a)]))
        # Status bar / header legibility over the top of the art.
        if layout["topLight"] != light:
            phone.paste(Image.new("RGB", (W, 48), scrim), (0, 0), mask(W, 48, [(0, 110), (1, 0)]))
    d = ImageDraw.Draw(phone)
    # Mock controls: title, artist, seek bar, transport, mode pills.
    y = W + 50 if style != "card" else 56 + W - 32 + 18
    d.rounded_rectangle((16, y, 150, y + 14), 4, fill=fg)
    d.rounded_rectangle((16, y + 22, 110, y + 32), 4, fill=tuple(int(v * 0.7 + (128 if fg[0] < 128 else 0) * 0.3) for v in fg))
    d.line((16, y + 56, W - 16, y + 56), fill=fg, width=2)
    d.rounded_rectangle((16, y + 74, 66, y + 110), 14, outline=fg, width=2)
    d.rounded_rectangle((74, y + 74, 166, y + 110), 14, fill=fg)
    d.rounded_rectangle((174, y + 74, W - 16, y + 110), 14, outline=fg, width=2)
    # Header text.
    d.rounded_rectangle((16, 30, 100, 40), 3, fill=fg)
    return phone


def main(src, out):
    os.makedirs(out, exist_ok=True)
    raw = os.path.join(out, "raw"); os.makedirs(raw, exist_ok=True)
    names = sorted(f for f in os.listdir(src) if not f.startswith("."))
    arts, manifest = {}, []
    for f in names:
        try:
            img = Image.open(os.path.join(src, f)).convert("RGB")
        except Exception:
            continue
        w, h = img.size
        s = DECODE / max(w, h)
        small = img.resize((max(1, round(w * s)), max(1, round(h * s))), Image.BOX)
        p = os.path.join(raw, f + ".rgba")
        open(p, "wb").write(small.convert("RGBA").tobytes())
        faces = faces_of(img)
        arts[f] = (img, faces)
        manifest.append(f"{p} {small.size[0]} {small.size[1]} " + json.dumps({"faces": faces, "preference": "automatic"}))
    res = subprocess.run([BIN], input="\n".join(manifest) + "\n", capture_output=True, text=True, check=True)
    layouts = [json.loads(l) for l in res.stdout.splitlines()]
    rows = [{"file": f, "faces": arts[f][1], "layout": l} for f, l in zip(arts, layouts)]
    with open(os.path.join(out, "layouts.jsonl"), "w") as fh:
        for r in rows:
            fh.write(json.dumps(r) + "\n")
    order = {"mirror": 0, "extend": 1, "card": 2}
    rows.sort(key=lambda r: (order[r["layout"]["bottom"]["style"]], r["layout"]["bottom"]["reason"], r["file"]))
    font = ImageFont.load_default()
    per, cols = 24, 8
    for n in range(0, len(rows), per):
        chunk = rows[n:n + per]
        sheet = Image.new("RGB", (cols * (W + 8) + 8, ((len(chunk) + cols - 1) // cols) * (H + 40) + 8), (40, 40, 40))
        d = ImageDraw.Draw(sheet)
        for i, r in enumerate(chunk):
            x, y = 8 + (i % cols) * (W + 8), 8 + (i // cols) * (H + 40)
            sheet.paste(render(arts[r["file"]][0], r["layout"]), (x, y))
            b = r["layout"]["bottom"]; m = b["metrics"]
            d.text((x, y + H + 2), f'{n + i}: {b["reason"]}  faces {m["faces"]}', fill=(255, 255, 255), font=font)
            d.text((x, y + H + 16), f'fl {m["flat"]:.2f} dr {m["drift"]:.3f} mk {m["marks"]} bz {m["busy"]:.3f}', fill=(200, 200, 200), font=font)
        sheet.save(os.path.join(out, f"sheet-{n // per:02d}.jpg"), quality=82)
    counts = {}
    for r in rows:
        k = r["layout"]["bottom"]["reason"]; counts[k] = counts.get(k, 0) + 1
    print(json.dumps(counts, indent=1))


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
