use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlicingRequest {
    pub engine: String,
    pub inputs: Vec<InputModel>,
    pub profile: Profile,
    pub bed: Bed,
    pub outputs: Outputs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputModel {
    pub path: String,
    pub transform: [f32; 16],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub nozzle_mm: f32,
    pub layer_h_mm: f32,
    pub material: String,
    pub speed_preset: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bed {
    pub size_mm: [f32; 3],
    pub origin: BedOrigin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BedOrigin {
    Min,
    Center,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outputs {
    pub gcode: String,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlicingResult {
    pub ok: bool,
    pub time_ms: u64,
    pub warnings: Vec<String>,
    pub preview: Option<PreviewSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewSummary {
    pub layers: u32,
    pub bbox: [f32; 6],
}

