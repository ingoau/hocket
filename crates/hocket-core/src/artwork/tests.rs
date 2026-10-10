use super::*;

const S: usize = 256;

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Canvas {
    fn new(w: usize, h: usize, rgb: [u8; 3]) -> Canvas {
        let mut px = Vec::with_capacity(w * h * 4);
        for _ in 0..w * h {
            px.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        Canvas { w, h, px }
    }

    fn set(&mut self, x: usize, y: usize, rgb: [u8; 3]) {
        let i = (y * self.w + x) * 4;
        self.px[i..i + 3].copy_from_slice(&rgb);
    }

    fn fill(&mut self, x0: usize, y0: usize, w: usize, h: usize, rgb: [u8; 3]) {
        for y in y0..(y0 + h).min(self.h) {
            for x in x0..(x0 + w).min(self.w) {
                self.set(x, y, rgb);
            }
        }
    }

    /// A line of "glyphs": thin dark strokes, like a caption, with its top at `y`.
    fn caption(&mut self, x: usize, y: usize, rgb: [u8; 3]) {
        for i in 0..8 {
            let gx = x + i * 14;
            self.fill(gx, y, 3, 16, rgb);
            self.fill(gx, y, 9, 3, rgb);
            self.fill(gx + 6, y + 6, 3, 10, rgb);
        }
    }

    fn layout(&self, preference: ImmersiveArtwork, faces: Vec<FaceRect>) -> ArtworkLayout {
        layout(
            &self.px,
            self.w as u32,
            self.h as u32,
            &ArtworkLayoutRequest { faces, preference },
        )
    }

    fn auto(&self) -> ArtworkLayout {
        self.layout(ImmersiveArtwork::Automatic, vec![])
    }
}

/// A soft, low-contrast pattern: something a reflection continues nicely.
fn soft_texture() -> Canvas {
    let mut c = Canvas::new(S, S, [0; 3]);
    for y in 0..S {
        for x in 0..S {
            let v = ((x as f32 / 9.0).sin() * (y as f32 / 11.0).cos() * 0.5 + 0.5) * 120.0;
            c.set(
                x,
                y,
                [20 + v as u8, 40 + (v * 0.6) as u8, 50 + (v * 0.8) as u8],
            );
        }
    }
    c
}

/// Deterministic high-contrast noise: a busy cover.
fn noise() -> Canvas {
    let mut c = Canvas::new(S, S, [0; 3]);
    let mut seed = 0x2545_f491_u32;
    for y in 0..S {
        for x in 0..S {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let [r, g, b, _] = seed.to_le_bytes();
            c.set(x, y, [r, g, b]);
        }
    }
    c
}

#[test]
fn a_soft_texture_is_mirrored() {
    let l = soft_texture().auto();
    assert_eq!(l.bottom.style, ArtworkStyle::Mirror, "{:?}", l.bottom);
    assert!(!l.bottom.light);
}

#[test]
fn text_near_the_edge_of_a_plain_cover_extends_its_colour_instead_of_mirroring() {
    // The "Do It" case: white, with a caption just above the bottom edge.
    let mut c = Canvas::new(S, S, [255; 3]);
    c.caption(120, S - 30, [10; 3]);
    let l = c.auto();
    assert_eq!(l.bottom.style, ArtworkStyle::Extend, "{:?}", l.bottom);
    assert_eq!(l.bottom.reason, "extend:flat");
    assert!(
        l.bottom.edge_colors.iter().all(|&c| c == 0xffffff),
        "{:x?}",
        l.bottom.edge_colors
    );
    // White stays white: dark controls, no scrim.
    assert!(l.bottom.light);
    assert!(l.bottom.scrim < 0.01, "{}", l.bottom.scrim);
}

#[test]
fn text_in_the_reflection_zone_rules_a_mirror_out() {
    let mut c = soft_texture();
    c.caption(40, S - 26, [255; 3]);
    let l = c.auto();
    assert_ne!(l.bottom.style, ArtworkStyle::Mirror, "{:?}", l.bottom);
    assert!(
        l.bottom.metrics.marks >= MARKS_MAX,
        "{:?}",
        l.bottom.metrics
    );
}

#[test]
fn a_busy_cover_is_a_card_unless_always() {
    let c = noise();
    let l = c.auto();
    assert_eq!(l.bottom.style, ArtworkStyle::Card, "{:?}", l.bottom);
    assert_eq!(
        c.layout(ImmersiveArtwork::Always, vec![]).bottom.style,
        ArtworkStyle::Extend
    );
    assert_eq!(
        c.layout(ImmersiveArtwork::Always, vec![]).bottom.reason,
        "extend:always"
    );
}

#[test]
fn never_is_always_a_card() {
    let l = soft_texture().layout(ImmersiveArtwork::Never, vec![]);
    assert_eq!(l.bottom.style, ArtworkStyle::Card);
    assert_eq!(l.right.style, ArtworkStyle::Card);
}

#[test]
fn a_face_rules_a_mirror_out_but_not_extending() {
    let face = FaceRect {
        x: 0.3,
        y: 0.2,
        w: 0.4,
        h: 0.4,
    };
    assert_ne!(
        soft_texture()
            .layout(ImmersiveArtwork::Automatic, vec![face])
            .bottom
            .style,
        ArtworkStyle::Mirror
    );
    // A portrait on a plain backdrop still extends.
    let mut c = Canvas::new(S, S, [200, 180, 160]);
    c.fill(90, 40, 80, 120, [90, 60, 50]);
    assert_eq!(
        c.layout(ImmersiveArtwork::Automatic, vec![face])
            .bottom
            .style,
        ArtworkStyle::Extend
    );
    // Tiny faces (a crowd far away) are ignored.
    let tiny = FaceRect {
        x: 0.5,
        y: 0.5,
        w: 0.06,
        h: 0.06,
    };
    assert_eq!(
        soft_texture()
            .layout(ImmersiveArtwork::Automatic, vec![tiny])
            .bottom
            .style,
        ArtworkStyle::Mirror
    );
}

#[test]
fn columns_that_hold_their_colour_extend_column_by_column() {
    // A left-to-right ramp, the same down every column, with a caption above the edge.
    let mut c = Canvas::new(S, S, [0; 3]);
    for y in 0..S {
        for x in 0..S {
            let v = (x * 255 / (S - 1)) as u8;
            c.set(x, y, [v, v / 2, 255 - v]);
        }
    }
    c.caption(60, S - 28, [255; 3]);
    let l = c.auto();
    assert_eq!(l.bottom.style, ArtworkStyle::Extend, "{:?}", l.bottom);
    assert_eq!(l.bottom.reason, "extend:columns");
    let first = l.bottom.edge_colors[0];
    let last = *l.bottom.edge_colors.last().unwrap();
    assert!(
        first & 0xff > 0xc0 && last >> 16 > 0xc0,
        "{first:06x} .. {last:06x}"
    );
}

#[test]
fn the_right_edge_is_decided_on_its_own() {
    // Busy on the left and bottom, a plain band down the right.
    let mut c = noise();
    c.fill(S - 60, 0, 60, S, [30, 60, 120]);
    let l = c.auto();
    assert_eq!(l.right.style, ArtworkStyle::Extend, "{:?}", l.right);
    assert_eq!(l.bottom.style, ArtworkStyle::Card, "{:?}", l.bottom);
    assert!(l
        .right
        .edge_colors
        .iter()
        .all(|&c| c == l.right.edge_colors[0]));
}

#[test]
fn a_non_square_cover_is_a_card() {
    let c = Canvas::new(S, S * 3 / 4, [255; 3]);
    assert_eq!(c.auto().bottom.reason, "card:notSquare");
    assert_eq!(
        c.layout(ImmersiveArtwork::Always, vec![]).bottom.style,
        ArtworkStyle::Card
    );
}

#[test]
fn a_dark_continuation_keeps_light_controls() {
    let l = Canvas::new(S, S, [12, 14, 20]).auto();
    assert_eq!(l.bottom.style, ArtworkStyle::Extend);
    assert!(!l.bottom.light);
    assert!(l.bottom.scrim < 0.01);
    assert!(!l.top_light);
}

#[test]
fn a_mid_tone_continuation_gets_a_scrim() {
    let l = Canvas::new(S, S, [125, 125, 125]).auto();
    assert!(l.bottom.scrim > 0.05, "{:?}", l.bottom);
}

#[test]
fn text_across_the_top_is_flagged() {
    let mut c = Canvas::new(S, S, [240, 235, 225]);
    c.caption(30, 6, [20; 3]);
    let l = c.auto();
    assert!(l.top_marks);
    assert!(l.top_light);
    assert!(!Canvas::new(S, S, [240, 235, 225]).auto().top_marks);
}

#[test]
fn bad_input_is_a_card() {
    assert_eq!(
        layout(&[0; 10], 4, 4, &ArtworkLayoutRequest::default())
            .bottom
            .style,
        ArtworkStyle::Card
    );
    assert_eq!(
        layout(&[], 0, 0, &ArtworkLayoutRequest::default())
            .right
            .style,
        ArtworkStyle::Card
    );
}

#[test]
fn small_covers_are_upsampled() {
    let mut c = Canvas::new(40, 40, [255; 3]);
    c.fill(0, 0, 40, 20, [0, 0, 0]);
    assert_eq!(c.auto().bottom.style, ArtworkStyle::Extend);
}

#[test]
fn json_round_trip() {
    let c = soft_texture();
    let out = layout_json(
        &c.px,
        S as u32,
        S as u32,
        r#"{"faces":[],"preference":"always"}"#,
    )
    .unwrap();
    let back: ArtworkLayout = serde_json::from_str(&out).unwrap();
    assert_eq!(back.bottom.style, ArtworkStyle::Mirror);
    assert!(out.contains("\"edgeColors\""));
    assert!(layout_json(&c.px, S as u32, S as u32, "").is_ok());
    assert!(layout_json(&c.px, S as u32, S as u32, "{").is_err());
}

#[test]
fn colour_round_trip() {
    for c in [[1.0, 0.0, 0.0], [0.2, 0.5, 0.9], [1.0, 1.0, 1.0]] {
        let back = srgb_of_lab(oklab_of_linear(c.map(srgb_to_linear)));
        for k in 0..3 {
            assert!((back[k] - c[k]).abs() < 0.002, "{c:?} -> {back:?}");
        }
    }
}
