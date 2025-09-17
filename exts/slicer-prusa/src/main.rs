use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use forge_protocol::SlicingRequest;
use tempfile::NamedTempFile;

#[derive(Debug, Parser)]
#[command(name = "prusa-adapter", about = "Maps SlicingRequest to prusa-slicer CLI")] 
struct Args {
    #[arg(long)]
    request: PathBuf,
    #[arg(long, default_value_t = true)]
    dry_run: bool,
    #[arg(long, default_value = "prusa-slicer")]
    bin: String,
}

#[derive(Debug, serde::Serialize)]
struct CmdPlan {
    bin: String,
    args: Vec<String>,
    load_config_path: Option<String>,
    notes: Vec<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let req: SlicingRequest = serde_json::from_slice(&std::fs::read(&args.request)?)
        .with_context(|| format!("reading request: {}", args.request.display()))?;

    let plan = build_prusaslicer_plan(&req, &args.bin)?;

    if args.dry_run {
        println!("{}", serde_json::to_string_pretty(&plan)?);
        return Ok(());
    }

    // Ensure output dirs exist
    if let Some(parent) = Path::new(&req.outputs.gcode).parent() { if !parent.as_os_str().is_empty() { std::fs::create_dir_all(parent)?; } }
    if let Some(parent) = Path::new(&req.outputs.preview).parent() { if !parent.as_os_str().is_empty() { std::fs::create_dir_all(parent)?; } }

    let status = Command::new(&plan.bin).args(&plan.args).status();
    match status {
        Ok(st) => {
            if !st.success() {
                return Err(anyhow!("prusa-slicer exited with status: {:?}", st));
            }
        }
        Err(e) => return Err(anyhow!("failed to spawn prusa-slicer: {}", e)),
    }
    // Try to normalize output: move generated gcode to requested path if needed
    let out_dir = prusaslicer_output_dir(&req);
    let expected = Path::new(&req.outputs.gcode);
    if !expected.exists() {
        if let Some(first) = req.inputs.first() {
            let base = Path::new(&first.path).file_stem().and_then(|s| s.to_str()).unwrap_or("job");
            let candidate = out_dir.join(format!("{}.gcode", base));
            if candidate.exists() {
                if let Some(parent) = expected.parent() { std::fs::create_dir_all(parent)?; }
                std::fs::rename(&candidate, &expected)?;
            }
        }
    }
    // Generate preview summary JSON
    if Path::new(&req.outputs.gcode).exists() {
        let _ = gcode_preview::summarize_gcode_to_preview(&req.outputs.gcode, &req.outputs.preview);
    }
    Ok(())
}

fn build_prusaslicer_plan(req: &SlicingRequest, bin_hint: &str) -> Result<CmdPlan> {
    if req.engine.to_lowercase() != "prusa-slicer" {
        return Err(anyhow!("unsupported engine for this adapter: {}", req.engine));
    }

    let bin = resolve_binary(bin_hint)?;

    let mut args: Vec<String> = Vec::new();

    args.push("-g".to_string());

    let out_dir = prusaslicer_output_dir(req);
    args.push("--output".to_string());
    args.push(out_dir.display().to_string());

    let mut notes = Vec::new();
    let config_ini = synth_config_ini(req);
    let mut load_config_path: Option<String> = None;
    if let Some(text) = config_ini {
        let mut tmp = NamedTempFile::new()?;
        std::io::Write::write_all(&mut tmp, text.as_bytes())?;
        let p = tmp.into_temp_path();
        let p_string = p.to_string_lossy().to_string();
        args.push("--load".to_string());
        args.push(p_string.clone());
        // Keep the temp file by persisting its path
        load_config_path = Some(p.to_path_buf().display().to_string());
        // Do not call remove_file here; let OS clean temp path later.
    }

    if req.inputs.is_empty() {
        return Err(anyhow!("no inputs provided"));
    }
    for input in &req.inputs {
        if !is_identity_transform(&input.transform) {
            notes.push(format!("transform for '{}' not applied in MVP", input.path));
        }
        args.push(input.path.clone());
    }

    Ok(CmdPlan {
        bin: bin.display().to_string(),
        args,
        load_config_path,
        notes,
    })
}

fn resolve_binary(bin_hint: &str) -> Result<PathBuf> {
    if let Ok(env_path) = std::env::var("PRUSA_SLICER_BIN") {
        let p = PathBuf::from(env_path);
        if p.exists() {
            return Ok(p);
        }
    }
    if let Ok(p) = which::which(bin_hint) {
        return Ok(p);
    }
    if let Ok(p) = which::which("PrusaSlicer") {
        return Ok(p);
    }
    Err(anyhow!("could not locate prusa-slicer binary; set PRUSA_SLICER_BIN or provide --bin"))
}

fn synth_config_ini(req: &SlicingRequest) -> Option<String> {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("layer_height={}", req.profile.layer_h_mm));
    lines.push(format!("nozzle_diameter={}", req.profile.nozzle_mm));
    lines.push(format!("filament_type={}", req.profile.material));
    Some(lines.join("\n"))
}

fn is_identity_transform(m: &[f32; 16]) -> bool {
    const I: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0,
        0.0, 1.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        0.0, 0.0, 0.0, 1.0,
    ];
    m.iter().zip(I.iter()).all(|(a,b)| (a-b).abs() < 1e-6)
}

fn prusaslicer_output_dir(req: &SlicingRequest) -> PathBuf {
    Path::new(&req.outputs.gcode).parent().map(|p| p.to_path_buf()).filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| PathBuf::from("."))
}
