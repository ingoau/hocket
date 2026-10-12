//! Runs the immersive-artwork classifier over raw covers, for tuning (see `scripts/artwork-eval.py`).
//!
//! Reads lines of `<path to raw RGBA8> <width> <height> <request JSON>` on stdin and writes one
//! `ArtworkLayout` JSON per line to stdout.
//!
//! ```text
//! cargo run -p hocket-core --example artwork_layout --release < manifest.txt > layouts.jsonl
//! ```
use std::io::{BufRead, Write};

fn main() -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let mut parts = line.splitn(4, ' ');
        let (Some(path), Some(w), Some(h)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let request = parts.next().unwrap_or("");
        let rgba = std::fs::read(path)?;
        writeln!(
            out,
            "{}",
            hocket_core::artwork::layout_json(&rgba, w.parse()?, h.parse()?, request)?
        )?;
    }
    Ok(())
}
