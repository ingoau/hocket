//! `hocket-mixlab`: the AutoMix harness (`docs/automix.md` → *Harness*).
//!
//! - `analyse <files..>`: full analysis records as JSON.
//! - `pair <a> <b>`: analyse both (records cached next to the files), plan,
//!   render 30 s of A into 30 s of B to WAV with the backends' own
//!   `render_tail` + `IncomingShaper`, and write the plan as JSON.
//! - `batch <pairs.txt>`: `pair` for every line of a pair list (track ids
//!   resolved in a corpus directory); can write the committed reference set.
//! - `stats <dir>`: plan random pairs and print the distribution of kinds,
//!   overlaps and cuts; `--check` also renders each one and checks levels.

mod render;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use rand::seq::SliceRandom;
use rand::SeedableRng;

use hocket_core::api::{AnalysisProducer, AnalysisRecord, TransitionKind, TransitionMode};
use hocket_core::automix::analysis::{apply_grid, Analyser, RawBeats};
use hocket_core::automix::plan::{Grid, Region};
use hocket_core::automix::{
    plan_transition, PlanInput, PlanTrack, TransitionPlan, TransitionSettings, ANALYSIS_VERSION,
};

#[derive(Parser)]
#[command(name = "hocket-mixlab", about = "AutoMix lab")]
struct Cli {
    /// Inference threads (0: one per core).
    #[arg(long, global = true, default_value_t = 0)]
    threads: usize,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, ValueEnum)]
enum Mode {
    Crossfade,
    Automix,
}

impl Mode {
    fn settings(self, crossfade_seconds: f64) -> TransitionSettings {
        TransitionSettings {
            mode: match self {
                Mode::Crossfade => TransitionMode::Crossfade,
                Mode::Automix => TransitionMode::AutoMix,
            },
            crossfade_seconds,
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Analyse files; print each record (or write `<out>/<stem>.json`).
    Analyse {
        files: Vec<PathBuf>,
        #[arg(long)]
        out: Option<PathBuf>,
        /// Print time and size per file to stderr.
        #[arg(long)]
        timing: bool,
        /// Use and fill the record cache next to each file (`<file>.automix.json`); print nothing.
        #[arg(long)]
        cache: bool,
    },
    /// Plan and render one transition.
    Pair {
        a: PathBuf,
        b: PathBuf,
        #[arg(long, value_enum, default_value = "automix")]
        mode: Mode,
        #[arg(long, default_value_t = 6.0)]
        crossfade: f64,
        #[arg(long, default_value = "mixlab-out")]
        out: PathBuf,
    },
    /// Plan and render every pair in a list (`<idA> <idB> [label]`, `#` comments).
    Batch {
        pairs: PathBuf,
        #[arg(long, default_value = "automix-corpus")]
        corpus: PathBuf,
        #[arg(long, default_value = "mixlab-out")]
        out: PathBuf,
        #[arg(long, value_enum, default_value = "automix")]
        mode: Mode,
        /// Skip rendering (plans only).
        #[arg(long)]
        no_render: bool,
        /// Also write the reference set: `<dir>/records/<id>.json` and `<dir>/plans/<a>_<b>.json`.
        #[arg(long)]
        reference: Option<PathBuf>,
    },
    /// Distribution of plans over random pairs of a directory's tracks.
    Stats {
        dir: PathBuf,
        #[arg(long, default_value_t = 200)]
        pairs: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long, value_enum, default_value = "automix")]
        mode: Mode,
        /// Render every pair in memory and check levels, NaNs and lengths.
        #[arg(long)]
        check: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut lab = Lab {
        threads: cli.threads,
        analyser: None,
    };
    match cli.cmd {
        Cmd::Analyse {
            files,
            out,
            timing,
            cache,
        } => {
            for f in files {
                if cache {
                    lab.record(&f)?;
                    continue;
                }
                let t = Instant::now();
                let r = lab.analyse(&f)?;
                if timing {
                    eprintln!(
                        "{}: {:.2} s for {:.0} s of audio, {} beats, bpm {:?}, confidence {:.2}",
                        f.display(),
                        t.elapsed().as_secs_f64(),
                        r.duration_ms as f64 / 1000.0,
                        r.beats_ms.len(),
                        r.bpm,
                        r.beat_confidence
                    );
                }
                let json = serde_json::to_string_pretty(&r)?;
                match &out {
                    Some(dir) => {
                        std::fs::create_dir_all(dir)?;
                        let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or("track");
                        std::fs::write(dir.join(format!("{stem}.json")), json)?;
                    }
                    None => println!("{json}"),
                }
            }
        }
        Cmd::Pair {
            a,
            b,
            mode,
            crossfade,
            out,
        } => {
            let settings = mode.settings(crossfade);
            lab.pair(&a, &b, &settings, &out, "pair", true)?;
        }
        Cmd::Batch {
            pairs,
            corpus,
            out,
            mode,
            no_render,
            reference,
        } => {
            let settings = mode.settings(6.0);
            for (ida, idb, label) in read_pairs(&pairs)? {
                let fa = find_track(&corpus, &ida)?;
                let fb = find_track(&corpus, &idb)?;
                let name = format!("{ida}_{idb}");
                eprintln!("{name} ({label})");
                let plan = lab.pair(&fa, &fb, &settings, &out, &name, !no_render)?;
                if let Some(dir) = &reference {
                    std::fs::create_dir_all(dir.join("records"))?;
                    std::fs::create_dir_all(dir.join("plans"))?;
                    for (id, f) in [(&ida, &fa), (&idb, &fb)] {
                        let r = lab.record(f)?;
                        std::fs::write(
                            dir.join("records").join(format!("{id}.json")),
                            serde_json::to_string_pretty(&r)? + "\n",
                        )?;
                    }
                    std::fs::write(
                        dir.join("plans").join(format!("{name}.json")),
                        serde_json::to_string_pretty(&plan)? + "\n",
                    )?;
                }
            }
        }
        Cmd::Stats {
            dir,
            pairs,
            seed,
            mode,
            check,
        } => lab.stats(&dir, pairs, seed, &mode.settings(6.0), check)?,
    }
    Ok(())
}

/// Lines of `<idA> <idB> [label...]`.
fn read_pairs(path: &Path) -> Result<Vec<(String, String, String)>> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut it = line.split_whitespace();
        if let (Some(a), Some(b)) = (it.next(), it.next()) {
            out.push((a.into(), b.into(), it.collect::<Vec<_>>().join(" ")));
        }
    }
    Ok(out)
}

fn is_audio(p: &Path) -> bool {
    matches!(
        p.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some(
            "flac" | "mp3" | "m4a" | "aac" | "ogg" | "opus" | "wav" | "aiff" | "aif" | "wv" | "mp4"
        )
    )
}

/// The corpus file of track `id` (`<id>.<suffix>`).
fn find_track(dir: &Path, id: &str) -> Result<PathBuf> {
    std::fs::read_dir(dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| is_audio(p) && p.file_stem().and_then(|s| s.to_str()) == Some(id))
        .ok_or_else(|| anyhow!("track {id} not in {}", dir.display()))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Cached {
    record: AnalysisRecord,
    raw: RawBeats,
}

fn cache_path(file: &Path) -> PathBuf {
    let mut s = file.as_os_str().to_owned();
    s.push(".automix.json");
    PathBuf::from(s)
}

struct Lab {
    threads: usize,
    analyser: Option<Analyser>,
}

impl Lab {
    fn analyse(&mut self, file: &Path) -> Result<AnalysisRecord> {
        if self.analyser.is_none() {
            self.analyser = Some(Analyser::new(self.threads)?);
        }
        let a = self.analyser.as_mut().expect("set");
        a.analyse_file(file, AnalysisProducer::Desktop)
            .with_context(|| file.display().to_string())
    }

    /// The record of `file`, from `<file>.automix.json` when it is current.
    /// The record of `file`. The cache next to it (`<file>.automix.json`)
    /// keeps the record and the tracker's raw output; the grid is rebuilt
    /// from the raw output every time, so grid post-processing changes need
    /// no re-analysis.
    fn record(&mut self, file: &Path) -> Result<AnalysisRecord> {
        let cache = cache_path(file);
        if let Ok(text) = std::fs::read_to_string(&cache) {
            if let Ok(mut c) = serde_json::from_str::<Cached>(&text) {
                if c.record.version == ANALYSIS_VERSION {
                    apply_grid(&mut c.record, &c.raw);
                    return Ok(c.record);
                }
            }
        }
        if self.analyser.is_none() {
            self.analyser = Some(Analyser::new(self.threads)?);
        }
        let t = Instant::now();
        let (record, raw) = self
            .analyser
            .as_mut()
            .expect("set")
            .analyse_file_raw(file, AnalysisProducer::Desktop)
            .with_context(|| file.display().to_string())?;
        eprintln!(
            "  analysed {} in {:.1} s",
            file.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
            t.elapsed().as_secs_f64()
        );
        let c = Cached { record, raw };
        std::fs::write(&cache, serde_json::to_string(&c)?)?;
        Ok(c.record)
    }

    fn plan(
        &mut self,
        a: &Path,
        b: &Path,
        settings: &TransitionSettings,
    ) -> Result<(AnalysisRecord, AnalysisRecord, TransitionPlan)> {
        let ra = self.record(a)?;
        let rb = self.record(b)?;
        let plan = plan_transition(&PlanInput {
            settings,
            a: PlanTrack {
                duration_ms: ra.duration_ms,
                record: Some(&ra),
            },
            b: PlanTrack {
                duration_ms: rb.duration_ms,
                record: Some(&rb),
            },
            fixed_tail_start_ms: None,
        })
        .ok_or_else(|| anyhow!("mode off"))?;
        Ok((ra, rb, plan))
    }

    fn pair(
        &mut self,
        a: &Path,
        b: &Path,
        settings: &TransitionSettings,
        out: &Path,
        name: &str,
        render: bool,
    ) -> Result<TransitionPlan> {
        let (ra, rb, plan) = self.plan(a, b, settings)?;
        std::fs::create_dir_all(out)?;
        std::fs::write(
            out.join(format!("{name}.plan.json")),
            serde_json::to_string_pretty(&plan)?,
        )?;
        println!("{name}: {}", describe(&plan, &ra, &rb));
        if render {
            let r = render::render_pair(a, b, &plan)?;
            println!("  {}", r.check.summary());
            r.check.verify()?;
            render::write_wav(&out.join(format!("{name}.wav")), &r.pcm, r.rate)?;
        }
        Ok(plan)
    }

    fn stats(
        &mut self,
        dir: &Path,
        n: usize,
        seed: u64,
        settings: &TransitionSettings,
        check: bool,
    ) -> Result<()> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| is_audio(p))
            .collect();
        files.sort();
        if files.len() < 2 {
            bail!("need at least two tracks in {}", dir.display());
        }
        let t = Instant::now();
        let mut records = Vec::new();
        for f in &files {
            records.push(self.record(f)?);
        }
        eprintln!(
            "{} records in {:.1} s",
            records.len(),
            t.elapsed().as_secs_f64()
        );

        let tails = records
            .iter()
            .filter(|r| Grid::of(r, Region::Tail(r.audible_end_ms)).is_some())
            .count();
        let heads = records
            .iter()
            .filter(|r| Grid::of(r, Region::Head(r.audible_start_ms)).is_some())
            .count();
        let confident = records.iter().filter(|r| r.beat_confidence >= 0.5).count();
        let mut conf: Vec<f32> = records.iter().map(|r| r.beat_confidence).collect();
        conf.sort_by(f32::total_cmp);
        println!(
            "tracks {}: confidence >= 0.5 {confident}, median confidence {:.2}; grid passes at tail {tails}, at head {heads}",
            records.len(),
            conf[conf.len() / 2],
        );

        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed);
        let mut idx: Vec<usize> = (0..files.len()).collect();
        let mut by_kind: BTreeMap<String, Vec<(f64, f64, f64, f64)>> = BTreeMap::new();
        let mut bars_hist: BTreeMap<String, usize> = BTreeMap::new();
        let mut checks = render::CheckTotals::default();
        for _ in 0..n {
            idx.shuffle(&mut rng);
            let (i, j) = (idx[0], idx[1]);
            let (ra, rb) = (&records[i], &records[j]);
            let plan = plan_transition(&PlanInput {
                settings,
                a: PlanTrack {
                    duration_ms: ra.duration_ms,
                    record: Some(ra),
                },
                b: PlanTrack {
                    duration_ms: rb.duration_ms,
                    record: Some(rb),
                },
                fixed_tail_start_ms: None,
            })
            .ok_or_else(|| anyhow!("mode off"))?;
            let overlap = plan.tail_len_ms.saturating_sub(plan.incoming_at_ms) as f64 / 1000.0;
            let cut_a = ra
                .audible_end_ms
                .saturating_sub(plan.tail_start_ms + plan.tail_source_ms)
                as f64
                / 1000.0;
            let skip_b = plan.incoming_from_ms.saturating_sub(rb.audible_start_ms) as f64 / 1000.0;
            let ratio = plan.tempo.map(|t| t.ratio_end as f64).unwrap_or(1.0);
            let kind = format!("{:?}", plan.kind);
            if let Some(g) = Grid::of(ra, Region::Tail(ra.audible_end_ms)) {
                let bars = (overlap * 1000.0 / g.bar_ms).round();
                *bars_hist
                    .entry(format!("{kind} {bars:>2} bars"))
                    .or_default() += 1;
            }
            by_kind
                .entry(kind)
                .or_default()
                .push((overlap, cut_a, skip_b, ratio));
            if check {
                let r = render::render_pair(&files[i], &files[j], &plan)?;
                checks.add(&r.check);
            }
        }
        println!("\n{n} random pairs, seed {seed}:");
        println!(
            "{:<11} {:>5} {:>6}  overlap s (min/med/p90/max)    A cut s (med/max)  B skip s (med/max)  |1-ratio| % (med/max)",
            "kind", "n", "share"
        );
        for (kind, v) in &by_kind {
            let col = |f: fn(&(f64, f64, f64, f64)) -> f64| {
                let mut c: Vec<f64> = v.iter().map(f).collect();
                c.sort_by(f64::total_cmp);
                c
            };
            let q = |c: &[f64], p: f64| c[((c.len() - 1) as f64 * p).round() as usize];
            let o = col(|x| x.0);
            let a = col(|x| x.1);
            let b = col(|x| x.2);
            let r = col(|x| (x.3 - 1.0).abs() * 100.0);
            println!(
                "{kind:<11} {:>5} {:>5.1}%  {:>5.1} {:>5.1} {:>5.1} {:>5.1}        {:>5.1} {:>5.1}        {:>5.1} {:>5.1}        {:>4.1} {:>4.1}",
                v.len(),
                100.0 * v.len() as f64 / n as f64,
                o[0],
                q(&o, 0.5),
                q(&o, 0.9),
                o[o.len() - 1],
                q(&a, 0.5),
                a[a.len() - 1],
                q(&b, 0.5),
                b[b.len() - 1],
                q(&r, 0.5),
                r[r.len() - 1],
            );
        }
        if !bars_hist.is_empty() {
            println!("\noverlap in A's bars (pairs where A's tail has a grid):");
            for (k, c) in &bars_hist {
                println!("  {k}: {c}");
            }
        }
        if check {
            println!("\n{}", checks.summary());
        }
        Ok(())
    }
}

fn describe(plan: &TransitionPlan, ra: &AnalysisRecord, rb: &AnalysisRecord) -> String {
    let kind = match plan.kind {
        TransitionKind::Fade => "Fade",
        TransitionKind::Blend => "Blend",
        TransitionKind::TempoBlend => "TempoBlend",
    };
    format!(
        "{kind}, overlap {:.1} s, tail from {:.1} s of {:.1} s, B from {:.1} s, bpm {:?} -> {:?}, conf {:.2}/{:.2}{}",
        plan.tail_len_ms.saturating_sub(plan.incoming_at_ms) as f64 / 1000.0,
        plan.tail_start_ms as f64 / 1000.0,
        ra.duration_ms as f64 / 1000.0,
        plan.incoming_from_ms as f64 / 1000.0,
        ra.bpm,
        rb.bpm,
        ra.beat_confidence,
        rb.beat_confidence,
        plan.tempo
            .map(|t| format!(", ratio {:.3}", t.ratio_end))
            .unwrap_or_default()
    )
}
