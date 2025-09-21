use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BedOrigin { #[default]
Min, Center }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BedSpec {
    pub size_mm: [f32; 3],
    pub origin: BedOrigin,
    #[serde(default)]
    pub front: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NozzleSpec {
    #[serde(default)]
    pub default_mm: Option<f32>,
    #[serde(default)]
    pub supported_mm: Option<Vec<f32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GCodeSpec {
    #[serde(default)]
    pub flavor: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub end: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LimitsSpec {
    #[serde(default)]
    pub vel_max: Option<Axis3>,
    #[serde(default)]
    pub accel_max: Option<Axis3>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Axis3 {
    pub xy: f32,
    pub z: f32,
    pub e: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeaturesSpec {
    #[serde(default)]
    pub has_auto_bed_level: bool,
    #[serde(default)]
    pub supports_binary_gcode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrinterProfile {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub bed: BedSpec,
    #[serde(default)]
    pub nozzle: NozzleSpec,
    #[serde(default)]
    pub gcode: GCodeSpec,
    #[serde(default)]
    pub limits: LimitsSpec,
    #[serde(default)]
    pub features: FeaturesSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FilamentTemps { pub first: i32, pub other: i32 }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoolingSpec {
    #[serde(default)]
    pub fan_percent: Option<FanCurve>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FanCurve { pub first: i32, pub other: i32, pub bridge: i32 }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FilamentProfile {
    pub id: String,
    pub material: String,
    pub diameter_mm: f32,
    #[serde(default)]
    pub temps: FilamentTempsSpec,
    #[serde(default)]
    pub cooling: CoolingSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FilamentTempsSpec {
    pub nozzle: FilamentTemps,
    pub bed: FilamentTemps,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SpeedSpec { pub walls: i32, pub infill: i32, pub travel: i32 }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrintSupportsSpec {
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub overhang_deg: Option<i32>,
    #[serde(default)]
    pub interface_layers: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrintProfile {
    pub id: String,
    pub layer_h_mm: f32,
    #[serde(default)]
    pub perimeters: Option<i32>,
    #[serde(default)]
    pub top_bottom: Option<TopBottomSpec>,
    #[serde(default)]
    pub infill: Option<InfillSpec>,
    #[serde(default)]
    pub speeds: Option<SpeedSpec>,
    #[serde(default)]
    pub supports: Option<PrintSupportsSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TopBottomSpec { pub top: i32, pub bottom: i32 }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InfillSpec { pub pattern: String, pub density: f32 }

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceOverride {
    pub id: String,
    #[serde(default)]
    pub printer: Option<String>,
    #[serde(default)]
    pub network: Option<NetworkSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkSpec { pub endpoint: String, #[serde(default)] pub token: Option<String> }

// The flattened effective settings needed to build a SlicingRequest
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EffectiveProfile {
    pub printer_id: String,
    pub filament_id: String,
    pub print_id: String,
    pub bed: BedSpec,
    pub nozzle_mm: f32,
    pub layer_h_mm: f32,
    #[serde(default)]
    pub gcode: GCodeSpec,
    #[serde(default)]
    pub temps: FilamentTempsSpec,
    #[serde(default)]
    pub speeds: Option<SpeedSpec>,
}

#[derive(Default)]
pub struct ConfigStore {
    pub root: PathBuf,
    pub printers: HashMap<String, PrinterProfile>,
    pub filaments: HashMap<String, FilamentProfile>,
    pub prints: HashMap<String, PrintProfile>,
    pub devices: HashMap<String, DeviceOverride>,
}

impl ConfigStore {
    pub fn load_default() -> Result<Self> {
        let home = dirs::home_dir().context("no home dir")?;
        let root = home.join(".forge");
        Self::load_from(&root)
    }

    pub fn load_from(root: &Path) -> Result<Self> {
        let mut s = ConfigStore { root: root.to_path_buf(), ..Default::default() };
        s.printers = load_dir::<PrinterProfile>(&root.join("printers"))?;
        s.filaments = load_dir::<FilamentProfile>(&root.join("filaments"))?;
        s.prints = load_dir::<PrintProfile>(&root.join("prints"))?;
        s.devices = load_dir::<DeviceOverride>(&root.join("devices"))?;
        Ok(s)
    }

    pub fn merge_for(&self, printer_id: &str, filament_id: &str, print_id: &str, device_id: Option<&str>) -> Result<EffectiveProfile> {
        let p = self.printers.get(printer_id).with_context(|| format!("unknown printer: {printer_id}"))?;
        let f = self.filaments.get(filament_id).with_context(|| format!("unknown filament: {filament_id}"))?;
        let pr = self.prints.get(print_id).with_context(|| format!("unknown print: {print_id}"))?;
        let _d = device_id.and_then(|id| self.devices.get(id));

        let nozzle_mm = p.nozzle.default_mm.unwrap_or(0.4);
        let eff = EffectiveProfile {
            printer_id: printer_id.to_string(),
            filament_id: filament_id.to_string(),
            print_id: print_id.to_string(),
            bed: p.bed.clone(),
            nozzle_mm,
            layer_h_mm: pr.layer_h_mm,
            gcode: p.gcode.clone(),
            temps: f.temps.clone(),
            speeds: pr.speeds.clone(),
        };
        Ok(eff)
    }
}

fn load_dir<T: for<'de> Deserialize<'de>>(dir: &Path) -> Result<HashMap<String, T>> {
    let mut out = HashMap::new();
    if !dir.exists() { return Ok(out); }
    for entry in glob::glob(&format!("{}/*.toml", dir.display()))? {
        if let Ok(path) = entry {
            let text = fs::read_to_string(&path)?;
            let cfg: T = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
            // try to read id field via toml again (cheap reuse) to extract `id`
            #[derive(Deserialize)]
            struct WithId { id: String }
            let WithId { id } = toml::from_str(&text).with_context(|| format!("reading id in {}", path.display()))?;
            out.insert(id, cfg);
        }
    }
    Ok(out)
}

