use anyhow::Result;
use forge_protocol::PreviewSummary;
use serde::Serialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Serialize)]
struct PreviewFile {
    preview: PreviewSummary,
}

pub fn summarize_gcode_to_preview<P: AsRef<Path>, Q: AsRef<Path>>(gcode: P, out_json: Q) -> Result<PreviewSummary> {
    let f = File::open(&gcode)?;
    let reader = BufReader::new(f);

    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];

    let mut cur_x: f32 = 0.0;
    let mut cur_y: f32 = 0.0;
    let mut cur_z: f32 = 0.0;
    let mut last_e: f32 = 0.0;
    let mut layers: u32 = 0;
    let mut last_layer_z: Option<f32> = None;

    for line in reader.lines() {
        let line = line?;
        let s = line.trim();
        if s.is_empty() || s.starts_with(';') {
            if let Some(rest) = s.strip_prefix(";LAYER:") {
                if rest.trim().chars().all(|c| c == '-' || c.is_ascii_digit()) { layers = layers.saturating_add(1); }
            }
            continue;
        }
        // crude parse of G0/G1 moves and extrusion
        if s.starts_with('G') && (s.starts_with("G0") || s.starts_with("G1")) {
            let mut nx = cur_x;
            let mut ny = cur_y;
            let mut nz = cur_z;
            let mut ne = last_e;
            for tok in s.split_whitespace().skip(1) {
                if tok.len() < 2 { continue; }
                let (axis, val) = tok.split_at(1);
                if let Ok(v) = val.parse::<f32>() {
                    match axis {
                        "X" => nx = v,
                        "Y" => ny = v,
                        "Z" => nz = v,
                        "E" => ne = v,
                        _ => {}
                    }
                }
            }
            // Count new layer when Z changes on extrusion moves
            if (nz - cur_z).abs() > 1e-4 {
                if last_layer_z.map(|lz| (nz - lz).abs() > 1e-4).unwrap_or(true) {
                    layers = layers.saturating_add(1);
                    last_layer_z = Some(nz);
                }
            }
            // If extruding, include bbox of the end point
            if ne > last_e + 1e-6 {
                min[0] = min[0].min(nx); min[1] = min[1].min(ny); min[2] = min[2].min(nz);
                max[0] = max[0].max(nx); max[1] = max[1].max(ny); max[2] = max[2].max(nz);
            }
            cur_x = nx; cur_y = ny; cur_z = nz; last_e = ne;
        }
    }

    // If no layers detected but bbox was touched, set layers to 1
    if !min[0].is_infinite() && layers == 0 { layers = 1; }

    let bbox = if min[0].is_infinite() {
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    } else {
        [min[0], min[1], min[2], max[0], max[1], max[2]]
    };

    let summary = PreviewSummary { layers, bbox };
    let pf = PreviewFile { preview: summary.clone() };
    std::fs::create_dir_all(out_json.as_ref().parent().unwrap_or_else(|| Path::new(".")))?;
    std::fs::write(out_json, serde_json::to_vec_pretty(&pf)?)?;
    Ok(summary)
}

