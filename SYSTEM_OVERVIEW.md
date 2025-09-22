## System Goals
1) Git-friendly: humans config TOML, machine artifacts in JSON; deterministic outputs
2) Extensible: everything beyond core editing/viewing is a plugin
3) Fleet-first: painless multi-printer dispatch and monitoring
4) Safe by default: guardrails on G-code; device actions gated by capabilities and lints

### Workspace and Storage Model
* Canonical rules: TOML for human-edited profiles; JSON for artifacts and scenes; 3MF only as an exchant format (import/export)
* Opinionated directory layout:

```
forge.toml                    # workspace meta, plugin manifest pins
schemas/                      # JSON Schemas (versioned)
printers/*.toml               # printer profiles (human)
filaments/*.toml              # filament profiles (human)
slicer_profiles/*.toml        # slicer presets (human)
plates/*.json                 # plate scenes (instances, transforms, modifiers)
models/**                     # STL/3MF sources (LFS recommended)
slices/<plate>/<hash>/*       # out.gcode, preview.bin, metrics.json
queue/*.json                  # queued tasks / dispatch plans (audit/replay)
cache/**                      # ignored
```

* Schema IDs: `forge.printer@1`, `forge.filament@1`, `forge.slicer_profile@1`, `forge.plate@1`
* Locking: `project.lock.json` pins plugin + engine versions for reproducibility

### Core Application Responsibility
* Document shell(bevy + egui): tabbed editor/views; command palette; problems/logs
* Schema-driven forms: render TOML/JSON editors from JSON schema + plugin FormSpecs
* Registry:
    * file association -> correct viewer/editor
    * plugin registry/capabilities -> route events/commands safely
* Undo/redo: global command log (every mutation is a command with an inverse)
* Job system: local queue + worker processes; cache keys for slices
* Fleet cache: background device state snapshots; instant tab switching

### Plugin runtime
* Control plane: JSON-RPC (stdio/uds); plugins are out of process with heartbeat, restart, and permissions
* Data plane: file URIs + SHA-256 (no inline blobs). Optional shared memory for high-rate local streams
* event bus -> commands:
    * events out: `project.opened`, `file.opened`, `printer.state_changed`, `slice.completed`, `lint.request`, etc
    * commands in: `apply_patch`, `enqueue(task)`, `open_tab`, `notify`, etc
* Plugin types:
    * device plugins: primarily printer, but could even handle things like robotic arms, ventilation, laser, or any kind of hardware driven by planning. Own the hardware; expose intents,
    state, and capabilities. No realtime servo control from host, soft limit of 1 Hz comms freq
    * Tooling plugins: slicer adapters, mesh analyzers, g-code lint/patches
    * UI contribution plugins: panels/forms via declarative specs; host renders
* Security: capability manifest per plugin: `read:workspace`, `write:workspace`, `network`, `device:<class>`, untrusted projects run with minimal rights

### Plate, Tab, and Dispatch Model
* PlateScene (JSON): printer-agnostic geometry + transforms + modifiers.
* TabBinding: `{ plate_id, printer_id, filament_id, slicer_profile_id, overrides }`
    * Tabs are bound; this fixes the pain point of having to flip profiles in a project between printers to fan out one plate to multiple printer models
* Matrix dispatch: build a DispatchPlan (JSON) selecting multiple printers/filaments; host expands to slice jobs (cache-aware) + dispatch jobs
* Determinism: slice cache key = hash(plate + profiles + engine + plugin version), normalize timestamps/comments in G-code for clean diffs

### Devices and Fleet
* Adapter per ecosystem:
    * Bambu LAN: Python plugin wrapped in JSON-RPC. State at 1-2 Hz (opt up to 10). Upload by hash; job submit/pause/cancel; AMS map
    * OctoPrint/Klipper/Moonraker (future concept): HTTP/WebSocket adapter
* State model: small JSON snapshots (temps, status, progress, AMS, alarms)
* eliminates "laggy" device switch: tabs read from cache, not live calls

### Slicing and Artifacts
* Workers run slicing/analysis off UI thread
* Artifacts live under `slices/<plate>/<hash>/...`:
    * `out.gcode` (normalized)
    * `preview.bin` (toolpath polylines, binary),
    * `metrics.json` (time/length/layer counts)
* Compare slices: diff metrics + key settings; show toolpath diffs via preview cache (not by parsing G-code again)

### G-code Viewer and Edits (Guarded)
* Viewer: streaming parse -> layer seek, color by speed/extruder/temp; stats and ETA
* Lints (example): forbidden M-codes per print, first-layer temp below minimum, unsafe travels without Z-hop, fan curve vs filament constraints
* Patches only: edits produce `*.patch.json` (set speed/fan curves, insert pause at layer, etc). Send path applies patch -> validates against capabilities -> dispatches
* Safety policy: "strict" blocks send if lints fail or caps mismatch

### Mesh Handling (Scoped)
* Non-destructive instance transforms stored in plate JSON
* Heavy ops (repair/decimate) are tooling tasks; outputs go to `models/generated/*.stl`
* Primitive insert only v1 (cube/cylinder/support blockers). No CSG booleans v1

### UI Layout
* Left side bar: explorer, extensions, source control, slice & print (queue + fleet), extensible with plugins
* Center: tabbed documents (3d plate, G-code, text/toml/json editors)
* Right drawer: context properties (schema-driven forms)
* Bottom: problems, logs/tasks
* Status bar: fleet summary (ready/busy, queue length), active profile, cache key hit/miss(?)

### Performance and Ops
* No blocking on bevy loop; all IO/parse/slice off thread
* STL/G-code stream parsing; GPU instancing for toolpath segments
* Printer polling consolidated; backoff on errors; delta emission
* Tests: golden cache keys, slice/dispatch replay, plugin simulation (record/replay)


### Safety and Trust
* Project trust modal (like VS code)
* Untrusted: plugins can't touch device/network
* Device plugins enforce local interlocks (E-stop/doors/vents) and expose only intents to host (planning-grade controls)
