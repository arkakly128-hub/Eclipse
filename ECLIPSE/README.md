# ECLIPSE

ECLIPSE is a from-scratch Rust game engine and game project for Windows, using
Direct3D 11. The engine is built around the needs of the game rather than a
general-purpose feature checklist.

> **Status:** First playable D3D11 scene.

## Project layout

- `engine/core/` — reusable engine systems: `renderer.rs` (D3D11), `window.rs`,
  `input.rs`, `mesh.rs`, `model_loader.rs`, `collision.rs`, and
  `music_system.rs` for shared music and sound playback, plus `asset_pack.rs`
  for indexed, streaming asset packs.
- `engine/core/Fculing.rs` — normalized camera frusta and world-space AABB tests.
- `engine/core/instancing.rs` — deterministic spatial instance batches with
  contiguous GPU instance ranges and per-batch bounds.
- `engine/core/Oculling.rs` — non-blocking D3D11 occlusion queries for tree
  batches; stale visibility is ignored when the camera moves.
- `engine/shader/` — HLSL shaders used by the engine. `Environment.hlsl`
  shades biome-textured PBR terrain, `world.hlsl` shades instanced trees,
  `Water.hlsl` draws tessellated Gerstner-wave water with Fresnel reflections,
  `Sky.hlsl` draws the atmospheric sky, `Fog.hlsl` composites depth haze and
  underwater tint, `Occlusion.hlsl` tests conservative proxy bounds, and
  `UI.hlsl` draws menu and HUD geometry.
- `engine/ui_layout.json` — the editable, shared 1280 × 720 layout for the
  menu, controls, HUD, inventory, and individual slots.
- `engine/ui_editor.html` — separate browser-based visual editor for that
  layout; it does not add an editor mode or hotkey to the game.
- `game/world/` — deterministic world seed/configuration and streamed chunks.
- `game/world/terrain.rs` — warped, blended biomes, coordinate-stable landforms,
  cached material-layer samples, and procedurally placed cabins.
- `game/world/chunk_cache.rs` — versioned, GZip-compressed terrain sample cache
  under `gamdata/chunks`.
- `game/world/river.rs` — meandering river channels, carved beds, and water meshes.
- `game/world/ravine.rs` — long, deep, dry canyons with steep banks.
- `game/world/ocean.rs` — continental shorelines, submerged terrain, and sea meshes.
- `game/world/water_ripple.rs` — a player-centered 32x32 ripple and flow simulation.
- `game/tree/tree.rs` — assigns `game/tree/tree.glb` to deterministic,
  instanced forest placement and trunk-only collision.
- `game/time/time.rs` — advances a smooth 24-hour day/night cycle and derives
  moving sun, moonlight, and time-dependent scene lighting.
- `game/inventory/inventory.rs` — player inventory data, stack handling, and
  the eight-slot hotbar.
- `game/settings/keybindings.rs` — the active keyboard bindings and their
  Controls-screen labels.
- `game/sound/` — game-side sound effect playback, with background music in
  `game/sound/music/` and sound effects in `game/sound/SFX/`. Release builds
  keep source files here; only audio files explicitly listed in
  `game/packaged_assets.txt` are copied into their category packs.
- `game/ui/ui.rs` — start menu state and UI input handling.
- `game/ui/ux.rs` — clean button layout and bitmap text rendering.
- `game/ui/hud.rs` — player health, stamina, food indicators, and hotbar.
- `game/colour.json` — editable `#RRGGBB` sun color and legacy ground/tree
  palette values; terrain surfaces use the physical maps in `texture.pak`.
- `game/colour/colour.rs` — loads and validates the color palette at startup.
- `game/player/player.rs` — player movement, swimming, breath, and drowning.
- `game/camera/camera.rs` — mouse-look and the player-following 3D view.
- `game/src/main.rs` — small game entry point that wires game systems together.
- `Cargo.toml` — Rust workspace.

## First world

The world is an integer-addressed chunk grid with floor-based world-to-chunk
conversion, including at negative coordinates. The seed in `world.cfg` drives
absolute-coordinate fractal noise and domain-warped, smoothly blended plains,
forest, desert, hills, snowy plains, and high snowy mountains. Reusing a seed
reproduces the same world; changing it reshapes the terrain, biome regions,
coastlines, rivers, ravines, and forests. Broad snowy lowlands can meet large
mountain ranges, with snow cover increasing toward high elevations. Ocean
basins transition through a sandy beach biome; river valleys and ravines have
their own terrain coloring. Terrain also includes mountain ranges,
meandering rivers, dry ravines, ocean basins, and occasional cabin landmarks.
Chunk size and active radius are configured in `world.cfg`; both the game and
standalone configs currently use **16 m chunks** and a **16-chunk radius**.
Chunks are generated in expanding square rings, at most four are installed per
frame, and a two-chunk padding pool keeps nearby generated chunks available
during travel.
Terrain and collision use the same generated mesh.

The first render target is a first-person view with Poly Haven mud, forest
ground, grass, rocky trail, sand, and snow materials, plus trees, rivers, a
sky, and directional sunlight. Biome boundaries blend neighboring materials;
the same snow set covers snowy plains and mountain peaks. Terrain uses packed
albedo, ARM (ambient occlusion/roughness/metallic), and AO maps under a
metallic-roughness lighting model. Water uses the supplied normal, roughness,
AO, and base-color maps along with its shared animated wave surface. River surfaces use
half-meter streamed geometry and oceans use meter-spaced geometry; both share
the same multi-wave Gerstner surface profile, shoreline clearance, and player
ripple simulation. The shader adds moving-surface normals, subtle Fresnel
reflections, and restrained sun glints. A damped 32x32
pressure-projected ripple simulation follows the player and adds splashes when
they enter or move through water. The Direct3D 11 renderer
uploads static terrain and imported tree-model buffers, frustum-culls terrain,
water, and spatial tree batches, and draws each visible tree batch with indexed
instancing. Asynchronous occlusion queries test tree-batch bounds against the
terrain/water depth buffer without stalling for query results.
The original 1K source maps are stored unchanged in `assets/texture.pak`.
They're decoded once at startup and uploaded at their original dimensions;
Direct3D generates their mip chains on the GPU once, with anisotropic filtering
for distant terrain. No per-frame CPU resizing or texture conversion occurs.
The renderer uses 2x multisample antialiasing on scene and UI targets, resolving
scene color for the depth-aware fog pass and resolving the completed frame
before presentation. Its standard PNG texture loader creates a complete mip
chain, and every vertex/pixel sampler slot receives a shared 16x anisotropic
wrap sampler during 3D passes, so future material shaders can use it
automatically. Screen-space UI keeps linear clamp filtering; button and slot
textures remain single-mip to preserve their edge detail.
Terrain shading uses world-space tiled material maps, biome blending, PBR
lighting, and AO in `Environment.hlsl`. Instanced tree geometry uses its own palette-aware
lighting in `world.hlsl`. `Sky.hlsl` draws the atmospheric gradient, sun,
clouds, and night details; `Fog.hlsl` reconstructs world distance from scene
depth and blends atmospheric haze, or a full-screen underwater tint with
animated caustics when the camera is submerged.
World data is generated only when a chunk is first streamed; rendering and
player movement do not generate terrain each frame. Terrain vertex heights,
normals, and biome colors are cached with a seed/coordinate/resolution/version
header and a generation-settings signature. GZip cache files are written via
temporary files on a background worker, validated on read, and the cache evicts
old files above 512 MiB. The cache is stored beside the executable at
`gamdata/chunks`; missing, stale, or invalid entries are regenerated. Terrain,
water, collision, and tree placement for new chunks are built on background
workers and streamed to the renderer in bounded batches.

The day/night cycle's length and starting time come from `day_length_seconds`
and `start_hour` in `game/world/world.cfg`. The sun travels to directly
overhead at noon, then sets; warm twilight, moonlight, night sky, water
reflections, and fog follow the same cycle.

The player is grounded against per-chunk static triangle-mesh BVHs and can walk
across chunk boundaries and descend ravine banks without a world-edge clamp.
In sufficiently deep water, Space swims up and Left Shift dives; underwater
vision adds depth haze and refraction. Air replenishes at the surface,
then drowning damages health when the 12-second breath reserve is depleted.
Spawn is seed-deterministic within three meters of the world origin and snaps to
the loaded terrain surface. Press **F3** to toggle the top-right world-position
coordinates. Spawn selection avoids steep ground, water channels, and ocean. Tree trunks use cheap
vertical-cylinder collision indexed by chunk; foliage remains non-collidable.
Spawn selection avoids steep ground and river channels, and walking is blocked
on terrain steeper than a 0.7 rise/run slope.
The HUD shows ten hearts plus stamina and food meters, with an air meter while
submerged. Their maximum units,
food depletion time, sprint/jump costs, regeneration, starvation damage, and
fall damage are set in `world.cfg`. The ten hearts divide the configured health
limit evenly. Stamina recovery consumes the configured additional food per
stamina unit; setting `food_depletion_seconds=0` disables passive food drain.
The game reads this file when it starts, so edits take effect on the next
launch without recompiling. For `cargo run`, edit `game/world/world.cfg`; for
the standalone build, edit `release/Eclipse/world.cfg`.
Press **E** to open the player inventory: 24 storage slots and the eight-slot
hotbar. Click slots to pick up, place, merge, or swap item stacks; number keys
continue to select the hotbar. The inventory starts empty because no items or
pickup sources are implemented yet. The inventory panel uses
`game/icons/inventory.png`, and inventory and hotbar cells use
`game/icons/slot.png`, with its editable vector source in `game/icons/slot.svg`
(included in `assets/icon.pak` for the release build).
HUD hearts use `game/icons/RED HEART.svg` and `game/icons/BLACK HEART.svg`,
rasterized directly at the 30 × 30 HUD icon size and included with their PNGs
in `assets/icon.pak`.
Cabins are visual landmarks for now. Player-built edits, gameplay/world-state
persistence, full character physics, and cast shadows are not implemented yet;
the generated-terrain cache is rebuildable and does not store player edits.

## UI layout editor

Open `engine/ui_editor.html` in a browser. Choose a screen, select an element
from the list or preview, then drag it to reposition it. Drag the selected
element's cyan corner to resize it, or enter exact X, Y, width, and height
values in the inspector. The editor includes all menu and controls elements,
HUD indicators, all 32 inventory slots, and each hotbar slot and number.
If you already changed the layout, use **Load layout** to open the current
`engine/ui_layout.json` before editing it.

Use **Save layout** and save/replace `engine/ui_layout.json`. If the browser
downloads the file instead of showing a save dialog, move the downloaded JSON
into the `engine` folder. Rebuild the game after saving; the standalone game
uses the saved layout at compile time and never opens the editor.

Edit the `colors` values in `game/colour.json` to change scene colors. Values
use `#RRGGBB` notation and are loaded when the game starts; additional color
entries can be added as the game gains more materials.

Edit `game/world/world.cfg` to tune world generation and gameplay. It includes
`max_health_units`, `max_stamina_units`, `max_food_units`,
`food_depletion_seconds`, `sprint_stamina_per_second`,
`jump_stamina_cost_units`, `stamina_regen_per_second`,
`food_per_stamina_unit`, `health_regen_per_second`,
`health_regen_food_per_second`, `starvation_damage_per_second`,
`fall_safe_distance_m`, and
`fall_damage_per_meter`, in addition to the day/night settings. The packaged
game reads the corresponding `release/Eclipse/world.cfg` file at startup.

To replace the tree model, put a self-contained `.glb` beside
`game/tree/tree.rs` and change `TREE_MODEL_PATH` there.
The static loader supports triangle meshes, node transforms, normals, vertex
colors, and opaque material base colors. Textures, transparency, animation, and
skinning are not supported yet; unsupported materials fail at startup with an
explicit error instead of silently rendering incorrectly.

## Build and run

On Windows with a Direct3D 11-capable GPU and the Rust toolchain installed, run:

```powershell
cargo test
cargo run
cargo run -p eclipse-game --bin pack_assets
```

`pack_assets` creates separate `icon.pak`, `sfx.pak`, `music.pak`, `model.pak`,
and `texture.pak` files in `release/Eclipse/assets/`. Packs are uncompressed and
indexed so textures can be read directly and audio can stream without loading
the complete archive. Keep source assets in `game/`; update
`game/packaged_assets.txt` to include only assets actually used by the game.
Do not copy the full SFX archives into the release.

After changing the UI layout, rebuild and refresh the standalone executable:

```powershell
cargo build --release -p eclipse-game
Copy-Item .\target\release\eclipse-game.exe .\release\Eclipse\game\Eclipse.exe -Force
```

## Standalone release

The ready-to-launch build is in `release/Eclipse/`. Double-click
`Eclipse.exe`; it loads `colour.json` beside the executable. UI textures,
sound effects, music, and the tree model are loaded from
`assets/icon.pak`, `assets/sfx.pak`, `assets/music.pak`, and `assets/model.pak`
respectively, with terrain and water maps loaded from `assets/texture.pak`.
`world.cfg` is stored beside `colour.json` and remains editable independently
of the executable. No Rust toolchain or Cargo command is required.

Controls: **WASD** move, **mouse** look, **Space** jump, **Left Ctrl** sneak,
**W/A/S/D** move, **Space** jump, **Left Shift** sprint, **Left Ctrl** crouch,
**E** open/close the inventory, **1–8** select a hotbar slot, and **Escape**
returns to the menu or closes a screen. Click **Play** to enter the world,
**Controls** to view the bindings, and **Quit** to exit.

## License

Not selected yet.
