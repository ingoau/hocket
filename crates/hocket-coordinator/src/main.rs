//! Headless coordinator binary. Owner: core-connect. See the crate docs in `lib.rs`.

use std::io::IsTerminal;

use clap::Parser;
use tracing::info;

use hocket_coordinator::{reports, serve, worker, App, Args, Command, ReportsCommand};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    if let Some(Command::Reports { action }) = &args.command {
        let dir = args
            .data_dir
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("reports need --data-dir"))?;
        let out = match action {
            ReportsCommand::List => reports::list(dir)?,
            ReportsCommand::Show { id } => reports::show(dir, id)?,
        };
        print!("{out}");
        return Ok(());
    }
    // The usual stderr log, plus the in-memory capture of the last hour that
    // `--diagnostics-user` accounts can receive (nothing is written to disk).
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info,tower_http=warn".into());
    hocket_core::devlog::install(&filter, None, std::io::stderr().is_terminal());
    let app = App::new(args.clone())?;
    // The opt-in analysis worker: its own thread, stopped with the server.
    let tmp_dir = match &args.data_dir {
        Some(dir) => dir.join("analysis-tmp"),
        None => std::env::temp_dir().join(format!("hocket-analysis-{}", std::process::id())),
    };
    let cancel = tokio_util::sync::CancellationToken::new();
    let worker = match (
        args.worker.resolve(tmp_dir, args.verify_url.as_deref())?,
        app.analysis_store(),
    ) {
        (Some(settings), Some(store)) => Some(worker::spawn(
            settings,
            store.clone(),
            app.worker_status_slot(),
            app.worker_priority()
                .expect("--analysis-server gives the app a priority queue"),
            cancel.clone(),
        )?),
        _ => None,
    };
    let shutdown = async {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            let mut term =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("signal");
            tokio::select! {
                _ = ctrl_c => {},
                _ = term.recv() => {},
            }
        }
        #[cfg(not(unix))]
        {
            let _ = ctrl_c.await;
        }
        info!("shutting down");
    };
    let (addr, handle) = serve(app.clone(), args.listen, shutdown).await?;
    info!(%addr, data_dir = ?args.data_dir, verify = !args.no_verify, "hocket-coordinator listening");
    handle.await?;
    // Flush every open room's replica on the way out.
    app.flush_all();
    if let Some(w) = worker {
        // A track being analysed finishes on its own time; don't wait long for it.
        cancel.cancel();
        let joined = tokio::task::spawn_blocking(move || w.join());
        if tokio::time::timeout(std::time::Duration::from_secs(3), joined)
            .await
            .is_err()
        {
            info!("analysis worker still busy; leaving it");
        }
    }
    Ok(())
}
