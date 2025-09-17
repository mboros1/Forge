pub mod prelude {
    pub use anyhow::{anyhow, Context, Result};
}

pub fn init_logging() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .try_init();
    });
}

use std::path::Path;
use std::process::Command;

pub fn dispatch_prusa_slicing(adapter_bin: &str, request_json: &Path, dry_run: bool) -> anyhow::Result<()> {
    let mut cmd = Command::new(adapter_bin);
    cmd.arg("--request").arg(request_json);
    if dry_run { cmd.arg("--dry-run"); }
    let status = cmd.status()?;
    if !status.success() { anyhow::bail!("adapter exited with status {:?}", status); }
    Ok(())
}
