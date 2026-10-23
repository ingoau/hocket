//! Headless coordinator binary. Owner: core-connect. See the crate docs in `lib.rs`.

use std::io::IsTerminal;

use clap::Parser;
use tracing::info;

use hocket_coordinator::{serve, App, Args};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // The usual stderr log, plus the in-memory capture of the last hour that
    // `--diagnostics-user` accounts can receive (nothing is written to disk).
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info,tower_http=warn".into());
    hocket_core::devlog::install(&filter, None, std::io::stderr().is_terminal());
    let args = Args::parse();
    let app = App::new(args.clone())?;
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
    Ok(())
}
