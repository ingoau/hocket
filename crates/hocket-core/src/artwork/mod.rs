//! Immersive artwork: how the full player shows a cover (docs/design.md, "Immersive artwork").
//!
//! [`layout`] is a pure function of a cover's pixels and the faces the platform found in it. The
//! platform decodes (it already does, for the artwork colours) and detects faces (Android's
//! `android.media.FaceDetector`); the decision lives here so both platforms make the same one and it
//! is tested once. Three styles, per edge:
//!
//! - [`ArtworkStyle::Mirror`] when a reflection would show nothing that looks wrong flipped: no
//!   faces, no text or logos near the edge, not so busy that the controls drown in it;
//! - [`ArtworkStyle::Extend`] when the edge is one colour (a plain background with text on it) or
//!   its columns are (a horizon, a gradient): its colours carry on, nothing is flipped;
//! - [`ArtworkStyle::Card`] otherwise.
//!
//! Everything is measured on the cover resampled to [`N`]×[`N`], in OKLab (so colour distances are
//! roughly perceptual); the thresholds were tuned on a real library's covers with the evaluation
//! example (`cargo run -p hocket-core --example artwork_layout`).

use crate::api::{
    ArtworkEdge, ArtworkLayout, ArtworkLayoutRequest, ArtworkMetrics, ArtworkStyle, FaceRect,
    ImmersiveArtwork,
};

#[cfg(test)]
mod tests;

/// Side of the square a cover is resampled to before measuring.
pub const N: usize = 128;
/// The strip along the edge that Extend continues (skipping the outermost row: borders, JPEG fringes).
const EDGE_ROWS: usize = 5;
/// The zone a reflection shows sharply (~12%); deeper than this it is blurred past reading.
const REFLECT_ROWS: usize = 16;
/// The zone whose busyness matters (~40%): what sits behind the controls, flipped.
const BUSY_ROWS: usize = 51;
/// The strip under the status bar and the player's header (~10%).
const TOP_ROWS: usize = 13;
/// Colours closer than this ([dist_flat]) count as the same flat colour.
const FLAT_TOL: f32 = 0.05;
/// An edge at least this flat is extended, never mirrored (the marks on it are ignored).
const FLAT_MIN: f64 = 0.85;
/// Local lightness contrast (OKLab L against the median of its surroundings) that makes a pixel part of a mark.
const MARK_CONTRAST: f32 = 0.16;
/// A mark's surroundings are plain (a background, not more texture) below this mean neighbour distance.
const PLAIN_NOISE: f32 = 0.03;
/// The coarse (large lettering) pass counts from this many marks.
const COARSE_MIN: u32 = 4;
/// The outermost row this share one colour: extend it, whatever is above (the art's own edge
/// carries on exactly, and nothing is flipped).
const EDGE_ROW_FLAT: f64 = 0.95;
/// This many small marks in the reflection zone rule a mirror out.
const MARKS_MAX: u32 = 2;
/// Busier than this (mean neighbour distance) rules a mirror out.
const BUSY_MAX: f64 = 0.045;
/// Column extension needs columns this steady and this smooth from one to the next.
const DRIFT_MAX: f64 = 0.03;
const ROUGH_MAX: f64 = 0.02;
/// Faces narrower than this share of the width are ignored: a figure seen whole reflects like
/// one standing by water; it is a close-up face that looks wrong upside down.
const FACE_MIN: f64 = 0.1;
/// How many colours [`ArtworkEdge::edge_colors`] carries.
const EDGE_SAMPLES: usize = 16;
/// Contrast the controls need over the continuation (WCAG AA for text).
const TARGET: f64 = 4.5;
/// The dark foreground used over light continuations (Material's light on-surface).
const DARK_FG: [f32; 3] = [
    0x1c as f32 / 255.0,
    0x1b as f32 / 255.0,
    0x1f as f32 / 255.0,
];

/// Layout for a cover given as tightly packed RGBA8 `width`×`height` pixels.
pub fn layout(
    rgba: &[u8],
    width: u32,
    height: u32,
    request: &ArtworkLayoutRequest,
) -> ArtworkLayout {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return card_layout("card:invalid");
    }
    let img = Img::from_rgba(rgba, w, h);
    let square = (w as f64 / h as f64 - 1.0).abs() <= 0.03;
    let faces: Vec<FaceRect> = request
        .faces
        .iter()
        .copied()
        .filter(|f| f.w >= FACE_MIN)
        .collect();
    let transposed_faces: Vec<FaceRect> = faces
        .iter()
        .map(|f| FaceRect {
            x: f.y,
            y: f.x,
            w: f.h,
            h: f.w,
        })
        .collect();
    let top = img.median(0..TOP_ROWS);
    ArtworkLayout {
        bottom: edge(&img, &faces, square, request.preference),
        right: edge(
            &img.transposed(),
            &transposed_faces,
            square,
            request.preference,
        ),
        top_light: luminance(top) > 0.4,
        top_marks: img.marks(0..TOP_ROWS) >= MARKS_MAX,
    }
}

/// [`layout`] over JSON, for the FFI crates: the request in, the layout out.
pub fn layout_json(
    rgba: &[u8],
    width: u32,
    height: u32,
    request_json: &str,
) -> Result<String, serde_json::Error> {
    let request: ArtworkLayoutRequest = if request_json.trim().is_empty() {
        ArtworkLayoutRequest::default()
    } else {
        serde_json::from_str(request_json)?
    };
    serde_json::to_string(&layout(rgba, width, height, &request))
}

fn card_layout(reason: &str) -> ArtworkLayout {
    let edge = ArtworkEdge {
        style: ArtworkStyle::Card,
        edge_colors: vec![0; EDGE_SAMPLES],
        base_color: 0,
        light: false,
        scrim: 0.0,
        reason: reason.into(),
        metrics: ArtworkMetrics::default(),
    };
    ArtworkLayout {
        bottom: edge.clone(),
        right: edge,
        top_light: false,
        top_marks: false,
    }
}

/// The decision for the bottom edge of `img` (the right edge is the bottom of the transposed image).
fn edge(img: &Img, faces: &[FaceRect], square: bool, preference: ImmersiveArtwork) -> ArtworkEdge {
    let strip = N - 1 - EDGE_ROWS..N - 1;
    // The outermost row alone (a few pixels of the original): one colour means a border, a
    // backdrop or a plain band, and the art can simply carry on in it.
    let last_median = img.median(N - 1..N);
    let last_row_flat = img.share_within(N - 1..N, last_median, FLAT_TOL) >= EDGE_ROW_FLAT;
    let median = if last_row_flat {
        last_median
    } else {
        img.median(strip.clone())
    };
    let flat = img.share_within(strip.clone(), median, FLAT_TOL);
    let columns = img.column_colours(strip.clone());
    let drift = img.drift(strip.clone(), &columns);
    let smooth = smooth_columns(&columns, 3);
    let rough = columns
        .iter()
        .zip(&smooth)
        .map(|(a, b)| dist_flat(*a, *b) as f64)
        .sum::<f64>()
        / N as f64;
    let marks = img.marks(N - REFLECT_ROWS..N);
    let busy = img.busyness(N - BUSY_ROWS..N);
    let face_count = faces.len() as u32;
    let metrics = ArtworkMetrics {
        flat,
        drift,
        rough,
        marks,
        busy,
        faces: face_count,
    };

    let mirror_ok = face_count == 0 && marks < MARKS_MAX && busy <= BUSY_MAX;
    // Only asked once the cheaper rules have not decided.
    let columns_ok = || drift <= DRIFT_MAX && rough <= ROUGH_MAX && img.marks(strip.clone()) == 0;
    let (style, reason): (ArtworkStyle, &str) = match preference {
        ImmersiveArtwork::Never => (ArtworkStyle::Card, "card:never"),
        _ if !square => (ArtworkStyle::Card, "card:notSquare"),
        // The art ends in one colour (a plain background, a border, a band, a backdrop, with or
        // without a caption or sticker on it): carry that colour on, never reflect it. Before the
        // mirror, so the choice doesn't hang on whether small text near the edge was seen.
        _ if flat >= FLAT_MIN || last_row_flat => (ArtworkStyle::Extend, "extend:flat"),
        _ if mirror_ok => (ArtworkStyle::Mirror, "mirror"),
        // Smearing a person's clothes or skin down the screen looks wrong: people only extend flat.
        _ if face_count == 0 && columns_ok() => (ArtworkStyle::Extend, "extend:columns"),
        ImmersiveArtwork::Always => (ArtworkStyle::Extend, "extend:always"),
        ImmersiveArtwork::Automatic if face_count > 0 => (ArtworkStyle::Card, "card:faces"),
        ImmersiveArtwork::Automatic if marks >= MARKS_MAX => (ArtworkStyle::Card, "card:marks"),
        ImmersiveArtwork::Automatic => (ArtworkStyle::Card, "card:busy"),
    };

    let flat_edge = reason == "extend:flat";
    // Extending a busy edge (the Always fallback) smears it: much smoother columns.
    let edge_cols = if flat_edge {
        vec![median; N]
    } else if reason == "extend:always" {
        smooth_columns(&columns, 16)
    } else {
        smooth
    };
    let edge_colors: Vec<[f32; 3]> = (0..EDGE_SAMPLES)
        .map(|i| {
            let span = &edge_cols[i * N / EDGE_SAMPLES..(i + 1) * N / EDGE_SAMPLES];
            mean_lab(span)
        })
        .collect();
    let (base, behind): ([f32; 3], Vec<[f32; 3]>) = match style {
        // The reflection under the controls is the lower part, flipped and blurred: block averages.
        ArtworkStyle::Mirror => (
            img.median(N - BUSY_ROWS..N),
            img.blocks(N - BUSY_ROWS..N, 16),
        ),
        _ => (
            if flat_edge {
                median
            } else {
                mean_lab(&edge_cols)
            },
            edge_colors.clone(),
        ),
    };
    let behind_rgb: Vec<[f32; 3]> = behind.iter().map(|c| srgb_of_lab(*c)).collect();
    let (light, scrim) = if style == ArtworkStyle::Card {
        (false, 0.0)
    } else {
        foreground(&behind_rgb)
    };
    ArtworkEdge {
        style,
        edge_colors: edge_colors.iter().map(|c| pack(srgb_of_lab(*c))).collect(),
        base_color: pack(srgb_of_lab(base)),
        light,
        scrim,
        reason: reason.into(),
        metrics,
    }
}

/// Light or dark controls, whichever needs the lighter scrim over `behind` (sRGB 0..1), and that scrim.
fn foreground(behind: &[[f32; 3]]) -> (bool, f64) {
    let need = |fg: [f32; 3], scrim: f32| -> f64 {
        let fg_l = luminance_srgb(fg);
        (0..=100)
            .map(|i| i as f32 / 100.0)
            .find(|&a| {
                behind.iter().all(|c| {
                    let over = [0, 1, 2].map(|k| c[k] * (1.0 - a) + scrim * a);
                    contrast(fg_l, luminance_srgb(over)) >= TARGET
                })
            })
            .unwrap_or(1.0) as f64
    };
    let white = need([1.0; 3], 0.0);
    let dark = need(DARK_FG, 1.0);
    if dark < white {
        (true, dark)
    } else {
        (false, white)
    }
}

fn contrast(a: f64, b: f64) -> f64 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

// ---------------------------------------------------------------------------
// The resampled image
// ---------------------------------------------------------------------------

/// A cover resampled to N×N, in OKLab, row-major.
struct Img {
    lab: Vec<[f32; 3]>,
}

impl Img {
    /// Box-filtered (area average, in linear light) from RGBA8; transparency composites over black.
    fn from_rgba(rgba: &[u8], w: usize, h: usize) -> Img {
        let lut: Vec<f32> = (0..256).map(|v| srgb_to_linear(v as f32 / 255.0)).collect();
        let mut lab = Vec::with_capacity(N * N);
        for dy in 0..N {
            let y0 = dy * h / N;
            let y1 = ((dy + 1) * h / N).max(y0 + 1).min(h);
            for dx in 0..N {
                let x0 = dx * w / N;
                let x1 = ((dx + 1) * w / N).max(x0 + 1).min(w);
                let mut acc = [0f32; 3];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let p = &rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
                        let a = p[3] as f32 / 255.0;
                        for k in 0..3 {
                            acc[k] += lut[p[k] as usize] * a;
                        }
                    }
                }
                let n = ((y1 - y0) * (x1 - x0)) as f32;
                lab.push(oklab_of_linear(acc.map(|v| v / n)));
            }
        }
        Img { lab }
    }

    fn transposed(&self) -> Img {
        let mut lab = vec![[0f32; 3]; N * N];
        for y in 0..N {
            for x in 0..N {
                lab[x * N + y] = self.lab[y * N + x];
            }
        }
        Img { lab }
    }

    fn at(&self, x: usize, y: usize) -> [f32; 3] {
        self.lab[y * N + x]
    }

    fn rows(&self, rows: std::ops::Range<usize>) -> impl Iterator<Item = [f32; 3]> + '_ {
        self.lab[rows.start * N..rows.end * N].iter().copied()
    }

    fn median(&self, rows: std::ops::Range<usize>) -> [f32; 3] {
        median_lab(self.rows(rows).collect())
    }

    fn share_within(&self, rows: std::ops::Range<usize>, c: [f32; 3], tol: f32) -> f64 {
        let (n, near) = self.rows(rows).fold((0, 0), |(n, near), p| {
            (n + 1, near + (dist_flat(p, c) <= tol) as usize)
        });
        near as f64 / n.max(1) as f64
    }

    fn column_colours(&self, rows: std::ops::Range<usize>) -> Vec<[f32; 3]> {
        (0..N)
            .map(|x| median_lab(rows.clone().map(|y| self.at(x, y)).collect()))
            .collect()
    }

    fn drift(&self, rows: std::ops::Range<usize>, columns: &[[f32; 3]]) -> f64 {
        let n = rows.len() * N;
        rows.flat_map(|y| (0..N).map(move |x| (x, y)))
            .map(|(x, y)| dist_flat(self.at(x, y), columns[x]) as f64)
            .sum::<f64>()
            / n.max(1) as f64
    }

    fn busyness(&self, rows: std::ops::Range<usize>) -> f64 {
        let mut sum = 0.0;
        let mut n = 0;
        for y in rows.start..rows.end.min(N - 1) {
            for x in 0..N - 1 {
                let p = self.at(x, y);
                sum += (dist(p, self.at(x + 1, y)) + dist(p, self.at(x, y + 1))) as f64 / 2.0;
                n += 1;
            }
        }
        sum / n.max(1) as f64
    }

    /// Mean colours of `size`×`size` blocks over `rows`.
    fn blocks(&self, rows: std::ops::Range<usize>, size: usize) -> Vec<[f32; 3]> {
        let mut out = Vec::new();
        let mut y = rows.start;
        while y < rows.end {
            let y1 = (y + size).min(rows.end);
            for bx in (0..N).step_by(size) {
                let cells: Vec<[f32; 3]> = (y..y1)
                    .flat_map(|yy| (bx..(bx + size).min(N)).map(move |xx| (xx, yy)))
                    .map(|(x, y)| self.at(x, y))
                    .collect();
                out.push(mean_lab(&cells));
            }
            y = y1;
        }
        out
    }

    /// Small, high-contrast shapes (glyphs, logos, stickers) in `rows`: connected groups of pixels
    /// whose lightness stands out from the median of their surroundings. A median keeps edges,
    /// so the boundary between two large areas never stands out; thin strokes and small shapes do.
    fn marks(&self, rows: std::ops::Range<usize>) -> u32 {
        // Two scales: body text and stickers, then large lettering whose strokes are thicker
        // than the small window (it would make the median itself). The coarse pass also picks up
        // the odd bold shape in a photo, so it only counts when it finds a word's worth.
        // Once the fine pass alone has ruled a mirror out, the coarse one cannot change anything.
        let fine = self.marks_at(rows.clone(), 3, N / 9);
        if fine >= MARKS_MAX {
            return fine;
        }
        let coarse = self.marks_at(rows, 7, N / 5);
        fine.max(if coarse >= COARSE_MIN { coarse } else { 0 })
    }

    fn marks_at(&self, rows: std::ops::Range<usize>, r: usize, max_h: usize) -> u32 {
        let mut window = Vec::with_capacity((2 * r + 1) * (2 * r + 1));
        let mut median = |x: usize, y: usize| {
            window.clear();
            for yy in y.saturating_sub(r)..(y + r + 1).min(N) {
                for xx in x.saturating_sub(r)..(x + r + 1).min(N) {
                    window.push(self.at(xx, yy)[0]);
                }
            }
            let mid = window.len() / 2;
            *window.select_nth_unstable_by(mid, f32::total_cmp).1
        };
        let (top, h) = (rows.start, rows.len());
        let mut on = vec![false; N * h];
        for y in rows.clone() {
            for x in 0..N {
                on[(y - top) * N + x] = (self.at(x, y)[0] - median(x, y)).abs() > MARK_CONTRAST;
            }
        }
        let mut seen = vec![false; N * h];
        let mut count = 0;
        let mut stack = Vec::new();
        for start in 0..N * h {
            if !on[start] || seen[start] {
                continue;
            }
            seen[start] = true;
            stack.push(start);
            let (mut x0, mut x1, mut y0, mut y1, mut area) = (N, 0, h, 0, 0);
            while let Some(i) = stack.pop() {
                let (x, y) = (i % N, i / N);
                area += 1;
                (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
                for (dx, dy) in [
                    (-1i32, -1i32),
                    (0, -1),
                    (1, -1),
                    (-1, 0),
                    (1, 0),
                    (-1, 1),
                    (0, 1),
                    (1, 1),
                ] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= N as i32 || ny >= h as i32 {
                        continue;
                    }
                    let j = ny as usize * N + nx as usize;
                    if on[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
            // A glyph or a small logo counts once; a run of glyphs that merged (bold text: the
            // gaps between letters stand out from a median the letters made bright) counts as a
            // few; anything taller than a line of text is a shape's detail, and a thin line right
            // across the cover is a line, not text.
            let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
            // Marks touching the outer half of the zone count double: a reflection shows those
            // sharpest (a Parental Advisory sticker in a bottom corner is enough on its own).
            if area >= 2 && bh <= max_h && self.plain_around(&on, top, h, (x0, y0, x1, y1)) {
                let weight = if top + y1 >= N - h / 2 && top > 0 {
                    2
                } else {
                    1
                };
                count += weight
                    * if bw <= N / 6 {
                        1
                    } else if bw <= N * 3 / 4 {
                        3
                    } else {
                        0
                    };
            }
        }
        count
    }

    /// Whether what surrounds a mark (its box grown by 2, minus the mark's own pixels) is smooth:
    /// text and logos sit on a clean background (a flat colour or a gentle gradient); the bright
    /// details of foliage, gravel or a crowd sit among more of themselves, and a reflection of
    /// those reads as texture, not as text.
    fn plain_around(
        &self,
        on: &[bool],
        top: usize,
        h: usize,
        (x0, y0, x1, y1): (usize, usize, usize, usize),
    ) -> bool {
        let (xa, xb, ya, yb) = (
            x0.saturating_sub(2),
            (x1 + 3).min(N),
            y0.saturating_sub(2),
            (y1 + 3).min(h),
        );
        let (mut sum, mut n) = (0f32, 0);
        for y in ya..yb {
            for x in xa..xb.saturating_sub(1) {
                if !on[y * N + x] && !on[y * N + x + 1] {
                    sum += dist(self.at(x, y + top), self.at(x + 1, y + top));
                    n += 1;
                }
            }
        }
        n > 0 && sum / n as f32 <= PLAIN_NOISE
    }
}

fn smooth_columns(columns: &[[f32; 3]], radius: usize) -> Vec<[f32; 3]> {
    (0..columns.len())
        .map(|x| mean_lab(&columns[x.saturating_sub(radius)..(x + radius + 1).min(columns.len())]))
        .collect()
}

fn median_lab(mut cells: Vec<[f32; 3]>) -> [f32; 3] {
    if cells.is_empty() {
        return [0.0; 3];
    }
    let mid = cells.len() / 2;
    [0, 1, 2].map(|k| {
        cells.sort_by(|a, b| a[k].total_cmp(&b[k]));
        cells[mid][k]
    })
}

fn mean_lab(cells: &[[f32; 3]]) -> [f32; 3] {
    let n = cells.len().max(1) as f32;
    [0, 1, 2].map(|k| cells.iter().map(|c| c[k]).sum::<f32>() / n)
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// [dist] with lightness through Ottosson's "toe" (the L_r of Okhsl), for asking whether colours
/// are the same flat colour: plain OKLab's cube root makes near-blacks far apart (sRGB 0 and 2
/// differ by 0.08 in L, more than a visible step elsewhere), which would call a pure black
/// background noisy. Texture and marks keep plain [dist], which they were tuned in.
fn dist_flat(a: [f32; 3], b: [f32; 3]) -> f32 {
    dist([toe(a[0]), a[1], a[2]], [toe(b[0]), b[1], b[2]])
}

const K1: f32 = 0.206;
const K2: f32 = 0.03;
const K3: f32 = (1.0 + K1) / (1.0 + K2);

fn toe(x: f32) -> f32 {
    0.5 * (K3 * x - K1 + ((K3 * x - K1) * (K3 * x - K1) + 4.0 * K2 * K3 * x).sqrt())
}

// ---------------------------------------------------------------------------
// Colour
// ---------------------------------------------------------------------------

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn oklab_of_linear([r, g, b]: [f32; 3]) -> [f32; 3] {
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn linear_of_oklab([l, a, b]: [f32; 3]) -> [f32; 3] {
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
        -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
    ]
}

fn srgb_of_lab(c: [f32; 3]) -> [f32; 3] {
    linear_of_oklab(c).map(linear_to_srgb)
}

/// WCAG relative luminance of an OKLab colour.
fn luminance(c: [f32; 3]) -> f64 {
    luminance_srgb(srgb_of_lab(c))
}

fn luminance_srgb(c: [f32; 3]) -> f64 {
    let [r, g, b] = c.map(|v| srgb_to_linear(v.clamp(0.0, 1.0)) as f64);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn pack(c: [f32; 3]) -> u32 {
    let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u32);
    (r << 16) | (g << 8) | b
}
