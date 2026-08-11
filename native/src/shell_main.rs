//! The Organon Shell binary (Shell #10 T1 + #14 T1): a terminal, with the
//! engine underneath it.
//!
//! PRD v3.1's form: a real PTY + the adopted VT core, drawn as a GPU glyph grid —
//! and, when summoned, the Organon renderer *behind* the glyphs (tree E Tier 1, the
//! first of the three lit-surface layers: engine-behind-the-terminal). The window
//! plumbing is the v2 lineage's (visual.rs's init, ui_layer.rs's paint order); the
//! device negotiation is `wgpu_editor::bring_up`'s in full — the #6-era debt repaid:
//! a default-limits device opens the window and then fails to create the engine's
//! pipelines, so the shell window always negotiates like the renderer host it is.
//!
//! Backdrop contract (PRD §4.6): summoned, never imposed — `ORGANON_SHELL_BACKDROP`
//! is tonight's dev summons (the typed `surface` command is tree E's real one), and
//! the legibility scrim over the render is not optional at any setting. The Console
//! Spike's Tier 1 gave that summons a second value: `1` is the live world, `substrate`
//! is one flat lit plane. See [`BackdropSource`].

use std::sync::Arc;

use organic_math_native::params::OrganicMathParams;
use organic_math_native::scene_input;
use organic_math_native::substrate_camera::SubstrateRig;
use organic_math_native::substrate_scene;
use organic_math_native::world::World;
use organon_core::edition::EDITION;
use organon_core::ipc;
use organon_shell::harness::{self, HarnessSpec};
use organon_shell::platform::Platform;
use organon_shell::session::SessionLog;
use organon_shell::tabs::{self, Tab, TabAction, TabStrip};
use organon_shell::term::TermSession;
use organon_shell::term_view;
use std::collections::HashSet;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// Swapchain + config. The `Device`/`Queue` are owned by the [`World`] after
/// `attach_gpu` and borrowed back for the egui pass — the route-C arrangement.
struct Gpu {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
}

/// The gamma arrangement measured in `wgpu_editor::SCENE_PANE_FORMAT`: render the
/// world through the sRGB format, hand egui a non-sRGB view of the same bytes —
/// egui's shader linearizes its samples itself, and a decoded-on-sample view would
/// linearize twice and come out dark.
const BACKDROP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const BACKDROP_SAMPLE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Console Spike Tier 1 — what fills the backdrop texture.
///
/// `ORGANON_SHELL_BACKDROP`'s value space is **widened, not replaced**: unset and `0` are
/// off, anything else is the World exactly as before, and one new spelling selects the lit
/// substrate plane. Keeping the World selectable is not politeness — the CLI's override lane
/// (`organon set`/`generator`/`recipe`) drains inside `World::frame_body`, so a substrate
/// that *replaced* the World would silently kill the live response the console demos
/// (brief R1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackdropSource {
    Off,
    World,
    Substrate,
}

/// The one new `ORGANON_SHELL_BACKDROP` value, quoted by the parser *and* by `--help` so the
/// two cannot drift — the discipline `SCRIM_DEFAULT` already earned here.
const BACKDROP_SUBSTRATE: &str = "substrate";

/// `ORGANON_SHELL_BACKDROP` → a source. `None` is "unset". Pure, so the value space is a
/// test rather than a claim.
fn parse_backdrop_source(v: Option<&str>) -> BackdropSource {
    match v {
        None | Some("0") => BackdropSource::Off,
        Some(s) if s.eq_ignore_ascii_case(BACKDROP_SUBSTRATE) => BackdropSource::Substrate,
        Some(_) => BackdropSource::World,
    }
}

/// The substrate lens, in **vertical** degrees — vertical is what the engine takes
/// (`world.rs:10564-10567`), and an axis mix-up is silent.
///
/// 10°, and the width is the deliverable rather than a framing detail: a flat plane under a
/// uniform material shades to one constant colour when the view vector does not vary (brief
/// R5), so the frustum's diagonal half-angle **is** the shading gradient. At 10° / 16:9 that
/// is `substrate_camera::max_view_deviation_deg` ≈ 10.1°. Narrower is now reachable — this
/// tier moved the engine's FOV clamp floor to 4° at both sites — and 4° frames the same plane
/// from ≈1023 world units with ≈4.1° of gradient. That headroom is deliberate and unspent: it
/// is the dial to turn if the backdrop reads as too much perspective.
const SUBSTRATE_FOV_DEG: f32 = 10.0;

/// The substrate plane's side in world units, **derived from the sheet the look actually
/// builds** rather than restated: `substrate_scene`'s lattice is `SUBSTRATE_GRID_X` nodes at
/// the membrane path's hard-coded 1-unit pitch, so it spans one less than that (127). Change
/// the grid and the framing follows.
const SUBSTRATE_EXTENT: f32 = substrate_scene::SUBSTRATE_GRID_X - 1.0;

/// The substrate key light's azimuth in degrees — **re-derived for the camera this file
/// installs**, and the one value of `substrate_scene`'s look that is overridden here.
///
/// Leaf B chose −10° against the *stock* camera (yaw 0.7 rad ≈ 40°, pitch 0.45), where it
/// reads as above-left, and says in as many words that the constant is coupled to whatever rig
/// the integrator installs. This rig is top-down (yaw 0, pitch ≈ π/2). Under
/// `look_at_rh(eye ≈ +Y·d, origin, Vec3::Y)` the screen basis comes out
/// **right = world +X, up = world −Z**: with the ε tilt aside the camera's up-vector has no
/// world Y left in it, so the key's *elevation* contributes nothing to where the light appears
/// to be and its azimuth alone decides the compass point. `dir_from_angles` builds the
/// direction **to** the light as `(cos e·sin a, sin e, cos e·cos a)`, which lands on screen at
/// `(sin a, −cos a)·cos e` — azimuth 0 reads bottom, 90 right, ±180 top, −90 left. Upper-left
/// at 45° is therefore **−135°**, and Leaf B's −10° would have read as lower-*left*: the same
/// light, a different camera. The derived fill follows for free at `a − 120° ≡ +105°`, from the
/// right and slightly above. In range (−180..180, `params.rs:8554`).
///
/// 📌 Checked, because it would have made this constant inert: `build_uniforms` **replaces**
/// `key_dir` with the terrain sun when the terrain backdrop is on with "sun lights scene". It
/// is gated on `terrain[0]`, and `terrain_enabled` defaults to **false** (`params.rs:8908`) —
/// `substrate_scene` writes neither, so the key stays ours. (That gate is the *terrain*
/// backdrop, not the atmosphere: `atmos_enabled` does default true and is exactly the sky this
/// rig wants for its IBL.)
///
/// It lives here and not in `substrate_scene.rs` because the coupling runs this way round: the
/// look is camera-agnostic, and the camera is this file's.
const SUBSTRATE_KEY_AZIMUTH_DEG: f32 = -135.0;

/// The `Shared` snapshot the console publishes every redraw, for a given backdrop source.
///
/// The substrate look is a **one-shot** write into it — the publisher (`redraw`'s
/// `w.write(*self.shared)`) then carries it every frame, so there is no per-frame substrate
/// path to keep in step. This function is the *only* place substrate state reaches the
/// snapshot, and it is a function rather than four lines inside `Shell::new` precisely so
/// "at any other source the bytes are exactly today's default look" is a test.
fn initial_shared(source: BackdropSource) -> Box<ipc::Shared> {
    let mut s = Box::new(OrganicMathParams::default().to_shared());
    if source == BackdropSource::Substrate {
        substrate_scene::apply_substrate_look(&mut s);
        // Last, and deliberately after the look: see [`SUBSTRATE_KEY_AZIMUTH_DEG`] for why
        // this one value is the camera's business and not the look's.
        s.lighting[4] = SUBSTRATE_KEY_AZIMUTH_DEG;
    }
    s
}

/// The engine's frame behind the glyphs: sized to the **pane it is painted into** (not the
/// window — see [`Shell::render_backdrop`]), recreated when that size changes, rebound to the
/// same egui id (`register_scene_texture`'s no-leak discipline).
struct Backdrop {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
    id: Option<egui::TextureId>,
}

struct Shell {
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
    renderer: Option<egui_wgpu::Renderer>,
    /// One PTY session per tab, index-aligned with `strip.tabs`. ALL sessions
    /// pump every frame (a background agent keeps streaming); only the active
    /// one draws. The 2026-08-08 reframe (PRD v3.2): Shell is a TUI HOST — the
    /// default tab runs the default HARNESS (Pi first among equals), and the
    /// bare terminal is one menu entry, not the opening position.
    sessions: Vec<TermSession>,
    strip: TabStrip,
    registry: Vec<HarnessSpec>,
    installed: HashSet<String>,
    default_harness: String,
    plus_open: bool,
    quit: bool,
    /// The engine (tree E). Owns the `Device`/`Queue` after `attach_gpu`; renders
    /// only into [`Shell::backdrop`], never the swapchain.
    world: World,
    backdrop: Option<Backdrop>,
    backdrop_source: BackdropSource,
    /// The terminal pane's size in **points** and the scale that turns it into physical
    /// pixels, recorded at the end of each frame and consumed by the next frame's
    /// [`Shell::render_backdrop`]. `None` until the first frame has laid the panel out.
    pane_points: Option<(f32, f32)>,
    pane_scale: f32,
    /// The `Shared` snapshot writer (organon-shell namespace). In the two-process
    /// design the PLUGIN writes this; Shell has no plugin, so the terminal writes
    /// the default look itself — which is what makes `organon status`/`get`/`watch`
    /// see a live system from inside the terminal, and gives the in-process world
    /// real params to read instead of zeroes. The CLI's override lane
    /// (`set`/`generator`/…) then applies on top in the world's working copy,
    /// exactly as it does against the standalone visual.
    shared_writer: Option<ipc::Writer>,
    shared: Box<ipc::Shared>,
    /// True while the surface acquires as `Occluded` — gates the redraw re-arm
    /// (the measured ~98%-CPU-drawing-nothing spin, fixed on the v2 branch).
    occluded: bool,
}

impl Shell {
    fn new() -> Self {
        let egui_ctx = egui::Context::default();
        egui_ctx.set_visuals(egui::Visuals::dark());
        let source =
            parse_backdrop_source(std::env::var("ORGANON_SHELL_BACKDROP").ok().as_deref());
        let shared = initial_shared(source);
        Self {
            window: None,
            gpu: None,
            egui_ctx,
            egui_state: None,
            renderer: None,
            sessions: Vec::new(),
            strip: TabStrip::default(),
            registry: Vec::new(),
            installed: HashSet::new(),
            default_harness: String::new(),
            plus_open: false,
            quit: false,
            world: World::new(),
            backdrop: None,
            backdrop_source: source,
            pane_points: None,
            pane_scale: 1.0,
            shared_writer: None,
            shared,
            occluded: false,
        }
    }

    fn init_gpu(&mut self, window: Arc<Window>) {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle().with_env());
        let surface = instance.create_surface(window.clone()).expect("create surface");
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .expect("request adapter");

        // The feature/limit negotiation is `wgpu_editor::bring_up`'s, in full: the
        // engine's cube pipeline needs `max_bind_groups` past wgpu's default of 4,
        // and RT/timestamps are what it probes for. A default-limits device opens
        // the window and then fails to create pipelines.
        let wanted = wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES
            | wgpu::Features::EXPERIMENTAL_RAY_QUERY
            | wgpu::Features::TIMESTAMP_QUERY
            | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        let coopmat_available = adapter
            .features()
            .contains(wgpu::Features::EXPERIMENTAL_COOPERATIVE_MATRIX);
        let f16_available = adapter.features().contains(wgpu::Features::SHADER_F16);

        let mut required_limits = wgpu::Limits::default();
        required_limits.max_bind_groups = adapter.limits().max_bind_groups;
        let required_features = adapter.features() & wanted;
        // wgpu gates EXPERIMENTAL_* behind an acknowledgement token on top of the
        // feature bit. SAFETY: wgpu's "there may be UB bugs in experimental APIs"
        // waiver; all ray-query use is contained in rt.rs (#195's churn rule).
        let experimental_features =
            if required_features.intersects(wgpu::Features::all_experimental_mask()) {
                unsafe { wgpu::ExperimentalFeatures::enabled() }
            } else {
                wgpu::ExperimentalFeatures::disabled()
            };
        if required_features.contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY) {
            let al = adapter.limits();
            required_limits.max_blas_primitive_count = al.max_blas_primitive_count;
            required_limits.max_blas_geometry_count = al.max_blas_geometry_count;
            required_limits.max_tlas_instance_count = al.max_tlas_instance_count;
            required_limits.max_acceleration_structures_per_shader_stage =
                al.max_acceleration_structures_per_shader_stage;
        }

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("organon-shell"),
            required_features,
            required_limits,
            experimental_features,
            ..Default::default()
        }))
        .expect("request device");

        let caps = surface.get_capabilities(&adapter);
        let format =
            caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: Default::default(),
        };
        surface.configure(&device, &config);

        self.renderer = Some(egui_wgpu::Renderer::new(
            &device,
            format,
            egui_wgpu::RendererOptions {
                msaa_samples: 1,
                depth_stencil_format: None,
                ..Default::default()
            },
        ));
        self.egui_state = Some(egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        ));
        // The world adopts the device (route C). Its only target here is the
        // backdrop texture, so its composite pipelines build for that format once.
        self.world.attach_gpu(
            device,
            queue,
            BACKDROP_FORMAT,
            None,
            coopmat_available,
            f16_available,
        );
        self.gpu = Some(Gpu { surface, config });
        self.window = Some(window);

        // Publish the default-look snapshot under this edition's namespace so the
        // CLI (and the world) find a live system. Republished each frame in
        // `redraw` — that heartbeat is what `organon watch` follows.
        match ipc::Writer::create() {
            Ok(w) => self.shared_writer = Some(w),
            Err(e) => eprintln!("organon-shell: Shared writer unavailable: {e}"),
        }

        // The registry: built-ins + the user's harnesses.json, detection by real
        // PATH probe. Default harness: $ORGANON_SHELL_DEFAULT if valid+installed,
        // else Pi if installed, else the plain shell — the TUI-host opening
        // position (PRD v3.2): your agent greets you, not a cursor.
        self.registry = SessionLog::store_root()
            .map(|r| harness::load(&r))
            .unwrap_or_else(harness::builtin);
        self.installed = harness::detect_installed(&self.registry, harness::on_path);
        self.default_harness = std::env::var("ORGANON_SHELL_DEFAULT")
            .ok()
            .filter(|id| self.installed.contains(id))
            .unwrap_or_else(|| {
                // Pi first (PRD §4.3), and on Windows the WSL entry is a real Pi —
                // usually the only one, since the toolchain lives in the distro.
                ["pi", "pi-wsl"]
                    .into_iter()
                    .find(|id| self.installed.contains(*id))
                    .unwrap_or("shell")
                    .to_string()
            });

        // Initial tabs: `ORGANON_SHELL_CMD` (dev hook, one plain-command tab),
        // else `ORGANON_SHELL_TABS` (comma-separated harness ids), else one tab
        // of the default harness.
        if let Ok(c) = std::env::var("ORGANON_SHELL_CMD") {
            self.open_tab_command(
                "sh".into(),
                Some(organon_shell::platform::shell_dash_c(Platform::current(), &c, |k| {
                    std::env::var(k).ok()
                })),
                None,
                "shell".into(),
            );
        } else {
            let ids = std::env::var("ORGANON_SHELL_TABS")
                .unwrap_or_else(|_| self.default_harness.clone());
            for id in ids.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                self.open_harness_tab(id);
            }
            if self.sessions.is_empty() {
                self.open_harness_tab("shell");
            }
        }
        self.strip.switch(0);
        self.sync_title();
    }

    /// Spawn a tab running `id`'s harness.
    ///
    /// The launch decision — which shell, how to wrap, whether it crosses into WSL,
    /// where its working directory applies — belongs to
    /// [`harness::launch_argv`], not here. This used to build
    /// `/bin/zsh -lc "exec …"` inline, which is why no harness could start on
    /// Windows.
    fn open_harness_tab(&mut self, id: &str) {
        let Some(spec) = self.registry.iter().find(|h| h.id == id).cloned() else {
            eprintln!("organon-shell: unknown harness {id:?}");
            return;
        };
        let (argv, cwd) = harness::launch_argv(
            &spec,
            Platform::current(),
            |k| std::env::var(k).ok(),
            harness::on_path,
        );
        self.open_tab_command(spec.name.clone(), Some(argv), cwd, spec.id.clone());
    }

    fn open_tab_command(
        &mut self,
        title: String,
        command: Option<Vec<String>>,
        cwd: Option<String>,
        hid: String,
    ) {
        match TermSession::spawn(80, 24, command, cwd.as_deref()) {
            Ok(s) => {
                self.sessions.push(s);
                self.strip.add(Tab { title, harness_id: hid });
            }
            // The failure a user actually hits is "this harness will not start", so
            // say what was tried, not just the OS error.
            Err(e) => eprintln!(
                "organon-shell: failed to spawn {title:?}: {e}\n  \
                 (harness {hid:?}; if this is a WSL entry, check `wsl.exe -- bash -lic 'command -v …'`)"
            ),
        }
    }

    fn sync_title(&self) {
        if let (Some(w), Some(tab)) = (self.window.as_ref(), self.strip.active_tab()) {
            w.set_title(&format!("{} — {}", tab.title, EDITION.product_name()));
        }
    }

    /// Apply one tab action after the egui frame — session lifetimes stay out of
    /// the closure, and closing the last tab quits (a terminal's convention).
    fn apply(&mut self, action: TabAction) {
        match action {
            TabAction::Switch(i) => self.strip.switch(i),
            TabAction::New(id) => self.open_harness_tab(&id),
            TabAction::Close(i) => {
                if i < self.sessions.len() {
                    self.sessions.remove(i);
                }
                if !self.strip.close(i) {
                    self.quit = true;
                }
            }
        }
        self.sync_title();
    }

    /// The engine's frame, sized to the pane it is painted into, behind the glyphs (tree E
    /// Tier 1; Console Spike Tier 1 fixed its aspect and gave it a second source).
    fn render_backdrop(&mut self) -> Option<egui::TextureId> {
        if self.backdrop_source == BackdropSource::Off {
            return None;
        }
        let swapchain = {
            let gpu = self.gpu.as_ref()?;
            (gpu.config.width.max(1), gpu.config.height.max(1))
        };
        // ⚠️ **The pane, not the window.** `term_view` paints this texture at UV 0..1 into the
        // CentralPanel's rect, which egui has already shrunk by the 30-point tab strip declared
        // ahead of it — so a swapchain-sized texture is stretched vertically by exactly that
        // strip. Brief R1 and R4 found the same defect from two directions; it is invisible on
        // a generative world and glaring on a flat plane, which is why it is fixed here in the
        // same tier that puts a plane behind the glyphs. This changes `BACKDROP=1`'s rendering
        // too, on purpose.
        //
        // One frame behind by construction — the world is drawn before the interface that
        // reserves its rect runs — and clamped rather than trusted, both exactly as
        // `wgpu_editor::render_scene_pane` does it. `pane_pixels` carries the clamps and their
        // reason: egui hands back a zero or negative rect for a frame mid-resize, and a
        // zero-sized texture is a validation error rather than a blank pane. Frame one has no
        // rect yet and falls back to the swapchain, i.e. to today's behaviour for one frame.
        let (w, h) = match self.pane_points {
            Some(pt) => scene_input::pane_pixels(pt, self.pane_scale),
            None => swapchain,
        };

        // Total over the source rather than only the substrate arm, so a runtime switch
        // (Tier 2's `organon console background`) needs no new wiring: the World arm actively
        // CLEARS the rig instead of leaving the camera framing a plane that is no longer
        // being drawn. Under `=1` this writes `None` over `None` every frame.
        if self.backdrop_source == BackdropSource::Substrate {
            // Re-framed every frame, which is how resize is handled without a staleness flag
            // to get wrong: the rig is computed for ONE aspect and the engine reads its own
            // from the render target, so a stale rig is a plane that no longer covers the
            // pane — Leaf A's re-frame warning. It costs six floats of trigonometry.
            let aspect = w as f32 / h.max(1) as f32;
            let rig = SubstrateRig::frame_plane(SUBSTRATE_EXTENT, SUBSTRATE_FOV_DEG, aspect);
            self.world.set_substrate_rig(Some(rig.camera_arm()));
        } else {
            self.world.set_substrate_rig(None);
        }

        let rebind = self.backdrop.as_ref().is_none_or(|b| b.size != (w, h));
        if rebind {
            let device = self.world.device()?;
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("shell-backdrop"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: BACKDROP_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[BACKDROP_SAMPLE_FORMAT],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("shell-backdrop-sample"),
                format: Some(BACKDROP_SAMPLE_FORMAT),
                ..Default::default()
            });
            let id = self.backdrop.as_ref().and_then(|b| b.id);
            self.backdrop = Some(Backdrop { texture, view, size: (w, h), id });
        }

        let pane = self.backdrop.as_ref()?;
        self.world.render_to_texture(&pane.texture, pane.size, BACKDROP_FORMAT);

        let device = self.world.device()?;
        let renderer = self.renderer.as_mut()?;
        let pane = self.backdrop.as_mut()?;
        match pane.id {
            Some(id) => {
                if rebind {
                    renderer.update_egui_texture_from_wgpu_texture(
                        device,
                        &pane.view,
                        wgpu::FilterMode::Linear,
                        id,
                    );
                }
                Some(id)
            }
            None => {
                let id =
                    renderer.register_native_texture(device, &pane.view, wgpu::FilterMode::Linear);
                pane.id = Some(id);
                Some(id)
            }
        }
    }

    fn redraw(&mut self) {
        let frame = {
            let (Some(window), Some(gpu)) = (self.window.as_ref(), self.gpu.as_mut()) else {
                return;
            };
            let Some(device) = self.world.device() else { return };
            match gpu.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(f)
                | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                wgpu::CurrentSurfaceTexture::Occluded => {
                    self.occluded = true;
                    return;
                }
                _ => {
                    gpu.surface.configure(device, &gpu.config);
                    window.request_redraw();
                    return;
                }
            }
        };

        // The plugin's job, done by the terminal: publish the snapshot the world
        // and the CLI read. Same bytes every frame (the default look) — the CLI
        // lane mutates the world's working copy, not this base, per #317's rule.
        if let Some(w) = self.shared_writer.as_mut() {
            w.write(*self.shared);
        }

        // The engine first, the terminal over it — the backdrop texture this frame
        // paints under the glyphs is the one just rendered.
        let backdrop = self.render_backdrop();

        let (Some(window), Some(gpu), Some(state), Some(renderer)) = (
            self.window.as_ref(),
            self.gpu.as_mut(),
            self.egui_state.as_mut(),
            self.renderer.as_mut(),
        ) else {
            return;
        };
        let Some(device) = self.world.device().cloned() else { return };
        let Some(queue) = self.world.queue().cloned() else { return };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let raw = state.take_egui_input(window);
        // Every tab pumps every frame — a background agent keeps streaming into
        // its grid; only the active one draws.
        for session in &mut self.sessions {
            session.pump();
        }
        let active = self.strip.active;
        let strip = &self.strip;
        let registry = &self.registry;
        let installed = &self.installed;
        let plus_open = &mut self.plus_open;
        let default_harness = self.default_harness.as_str();
        let sessions = &mut self.sessions;
        let mut action: Option<TabAction> = None;
        // The rect the terminal actually paints into, captured for the NEXT frame's backdrop
        // (see `render_backdrop`). Taken from the same `ui` and by the same call
        // `term_view::draw` sizes its grid from, so the texture and the quad cannot disagree.
        let mut pane_rect: Option<egui::Rect> = None;
        let out = self.egui_ctx.run(raw, |ctx| {
            // ⌘-keys are the host's chrome (term_view skips them for the PTY).
            ctx.input(|i| {
                for ev in &i.events {
                    if let egui::Event::Key { key, pressed: true, modifiers, .. } = ev {
                        if action.is_none() {
                            action =
                                tabs::command_key_action(*key, *modifiers, strip, default_harness);
                        }
                    }
                }
            });
            // The tab strip: the one permitted chrome (FR-T11), Superconductor's
            // form factor — along the top, + menu with the numbered registry.
            egui::TopBottomPanel::top("tab-strip")
                .exact_height(30.0)
                .frame(egui::Frame::NONE.fill(egui::Color32::from_rgb(0x07, 0x09, 0x07)))
                .show(ctx, |ui| {
                    if let Some(a) = tabs::tab_bar(ui, strip, registry, installed, plus_open) {
                        action = Some(a);
                    }
                });
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(term_view::DEFAULT_BG))
                .show(ctx, |ui| {
                    // Before anything is allocated in it — `term_view::draw`'s own first act
                    // is this same call.
                    pane_rect = Some(ui.available_rect_before_wrap());
                    if let Some(session) = sessions.get_mut(active) {
                        term_view::draw(ui, session, backdrop);
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.monospace("no live tab — ⌘T opens one");
                        });
                    }
                });
        });
        state.handle_platform_output(window, out.platform_output);
        // What the next frame's backdrop is sized to. Points plus the scale, never pixels:
        // the conversion belongs with the clamps in `pane_pixels`.
        self.pane_points = pane_rect.map(|r| (r.width(), r.height()));
        self.pane_scale = out.pixels_per_point;
        if let Some(action) = action {
            self.apply(action);
        }
        let (Some(window), Some(gpu), Some(state), Some(renderer)) = (
            self.window.as_ref(),
            self.gpu.as_mut(),
            self.egui_state.as_mut(),
            self.renderer.as_mut(),
        ) else {
            return;
        };
        let _ = state;

        let jobs = self.egui_ctx.tessellate(out.shapes, out.pixels_per_point);
        let sd = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [gpu.config.width, gpu.config.height],
            pixels_per_point: out.pixels_per_point,
        };
        for (id, delta) in &out.textures_delta.set {
            renderer.update_texture(&device, &queue, *id, delta);
        }
        let mut encoder = device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("shell-ui") });
        let mut staged = renderer.update_buffers(&device, &queue, &mut encoder, &jobs, &sd);
        {
            let mut rp = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shell-ui-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.003,
                                g: 0.004,
                                b: 0.003,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            renderer.render(&mut rp, &jobs, &sd);
        }
        // Staging buffers first, then our draws — the order ui_layer.rs documents.
        staged.push(encoder.finish());
        queue.submit(staged);
        // wgpu 30 moved `present` onto the queue (the World::present precedent).
        queue.present(frame);
        for id in &out.textures_delta.free {
            renderer.free_texture(id);
        }
        window.request_redraw();
    }
}

impl ApplicationHandler for Shell {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(EDITION.product_name())
            .with_inner_size(winit::dpi::LogicalSize::new(1100.0, 720.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        self.init_gpu(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::Occluded(hidden) = event {
            self.occluded = hidden;
            if !hidden {
                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
        }
        if let (Some(window), Some(state)) = (self.window.as_ref(), self.egui_state.as_mut()) {
            let response = state.on_window_event(window, &event);
            if response.repaint && !self.occluded {
                window.request_redraw();
            }
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let (Some(gpu), Some(device)) = (self.gpu.as_mut(), self.world.device()) {
                    gpu.config.width = size.width.max(1);
                    gpu.config.height = size.height.max(1);
                    gpu.surface.configure(device, &gpu.config);
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
                if self.quit {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
}

/// What the command line asked for, as a value.
///
/// Pure so the decision is unit-tested without a window server — the same reason
/// [`organon_shell::platform::Platform`] is a value rather than a `#[cfg]`.
#[derive(Debug, PartialEq, Eq)]
enum Invocation {
    Help,
    Version,
    Run,
}

/// ⚠️ **`organon-shell --help` used to hang forever.** There was no argument handling at
/// all: the flag was ignored, the banner printed, and the winit event loop started — so the
/// obvious way to probe a new product ate the terminal until the user found the window or
/// killed it. The first public-repo trial gave up after three minutes. Every other binary in
/// this tree answers `--help`; this one is a GUI app, which is a reason to keep the answer
/// short, not a reason not to answer.
fn invocation(args: &[String]) -> Invocation {
    for a in args.iter().skip(1) {
        match a.as_str() {
            "-h" | "--help" | "help" => return Invocation::Help,
            "-V" | "--version" => return Invocation::Version,
            _ => {}
        }
    }
    Invocation::Run
}

/// The interface is environment variables, not flags, so `--help` documents *those*.
/// Listing flags this binary does not have would be worse than the silence it replaces.
fn help_text() -> String {
    format!(
        "{} — {}\n\
         \n\
         Usage: organon-shell            (no flags; the surface is environment variables)\n\
         \n\
         Options:\n    \
             -h, --help       print this and exit\n    \
             -V, --version    print the version and exit\n\
         \n\
         Environment:\n    \
             ORGANON_SHELL_BACKDROP=<src> behind the glyphs: 0/unset off, 1 the world,\n                                 \
             {substrate} the lit substrate plane\n    \
             ORGANON_SHELL_SCRIM=<0..255> legibility scrim alpha (default {scrim_default}, floor {scrim_floor})\n    \
             ORGANON_SHELL_TABS=a,b,c     open these harness ids at start\n    \
             ORGANON_SHELL_DEFAULT=<id>   harness for the first tab (else Pi if installed)\n    \
             ORGANON_SHELL_CMD=<cmd>      one plain-command tab, for headless checks\n    \
             ORGANON_SHELL_PTY_DEBUG=1    trace the PTY byte path to stderr ([pty]/[grid])\n    \
             ORGANON_IPC_NS=<name>        IPC namespace; fork it to run beside another Organon\n\
         \n\
         Inside a tab the `organon` CLI addresses this process — the namespace is inherited.\n\
         Docs: SHELL_ARCHITECTURE.md\n",
        EDITION.product_name(),
        EDITION.tagline(),
        substrate = BACKDROP_SUBSTRATE,
        scrim_default = term_view::SCRIM_DEFAULT,
        scrim_floor = term_view::SCRIM_FLOOR,
    )
}

fn main() {
    match invocation(&std::env::args().collect::<Vec<_>>()) {
        Invocation::Help => {
            print!("{}", help_text());
            return;
        }
        Invocation::Version => {
            println!("{} {}", EDITION.product_name(), env!("CARGO_PKG_VERSION"));
            return;
        }
        Invocation::Run => {}
    }

    eprintln!("{} — {}", EDITION.product_name(), EDITION.tagline());
    let event_loop = EventLoop::new().expect("event loop");
    let mut shell = Shell::new();
    event_loop.run_app(&mut shell).expect("run app");
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn help_and_version_are_answered_not_swallowed() {
        for spelling in [v(&["organon-shell", "--help"]), v(&["organon-shell", "-h"])] {
            assert_eq!(invocation(&spelling), Invocation::Help, "{spelling:?}");
        }
        for spelling in [v(&["organon-shell", "--version"]), v(&["organon-shell", "-V"])] {
            assert_eq!(invocation(&spelling), Invocation::Version, "{spelling:?}");
        }
    }

    /// argv[0] is a path, and on Windows it can be `...\organon-shell.exe` — neither may be
    /// mistaken for a flag, or the app would print help instead of starting.
    #[test]
    fn argv0_is_never_read_as_a_flag() {
        assert_eq!(invocation(&v(&["/usr/local/bin/organon-shell"])), Invocation::Run);
        assert_eq!(invocation(&v(&[r"C:\tools\help\organon-shell.exe"])), Invocation::Run);
        assert_eq!(invocation(&v(&["organon-shell"])), Invocation::Run);
    }

    /// The scrim line is quoted from `term_view`'s constants, not restated — the first draft
    /// said `<0..1>` from memory, and `0.5` fails the `u8` parse, gets swallowed by `.ok()`
    /// and silently falls back. This pins both halves: the byte scale in the notation, and
    /// the actual numbers. Change `SCRIM_DEFAULT`/`SCRIM_FLOOR` and the help follows; write a
    /// literal back into the help and this fails.
    #[test]
    fn the_scrim_line_matches_the_code_it_documents() {
        let h = help_text();
        assert!(h.contains("ORGANON_SHELL_SCRIM=<0..255>"), "scrim scale is a u8, not 0..1");
        assert!(!h.contains("<0..1>"), "the 0..1 form silently no-ops — never document it");
        assert!(
            h.contains(&format!("default {}", organon_shell::term_view::SCRIM_DEFAULT)),
            "help does not quote SCRIM_DEFAULT"
        );
        assert!(
            h.contains(&format!("floor {}", organon_shell::term_view::SCRIM_FLOOR)),
            "help does not quote SCRIM_FLOOR"
        );
    }

    /// The backdrop's value space, both halves: the new spelling reaches the substrate, and
    /// **every other value still means what it meant before**. That second half is the point —
    /// Tier 1 widened `ORGANON_SHELL_BACKDROP` rather than redefining it, and a console that
    /// quietly stopped rendering the World would take the CLI's live override lane with it.
    #[test]
    fn the_backdrop_value_space_is_widened_not_redefined() {
        assert_eq!(parse_backdrop_source(None), BackdropSource::Off);
        assert_eq!(parse_backdrop_source(Some("0")), BackdropSource::Off);
        assert_eq!(parse_backdrop_source(Some(BACKDROP_SUBSTRATE)), BackdropSource::Substrate);
        assert_eq!(parse_backdrop_source(Some("SUBSTRATE")), BackdropSource::Substrate);
        // Every spelling that was "on" before is still the World, including the ones nobody
        // types: the old rule was literally `!= "0"`.
        for on in ["1", "2", "", "true", "yes", "substrat", "substratee"] {
            assert_eq!(parse_backdrop_source(Some(on)), BackdropSource::World, "{on:?}");
        }
    }

    /// **`ORGANON_SHELL_BACKDROP=1` publishes today's bytes, unchanged.** The snapshot the
    /// console writes every redraw is what `organon status`/`get`/`watch` read and what the
    /// world renders from, so a substrate write leaking into the World source would change the
    /// product's whole default look — quietly, since it would still render something. The
    /// claim is checked against the raw bytes rather than a field list, which is the only form
    /// that cannot be outgrown by a future look constant.
    #[test]
    fn only_the_substrate_source_touches_the_published_snapshot() {
        let base = OrganicMathParams::default().to_shared();
        for src in [BackdropSource::Off, BackdropSource::World] {
            assert_eq!(
                bytemuck::bytes_of(&*initial_shared(src)),
                bytemuck::bytes_of(&base),
                "{src:?} must publish exactly today's default look"
            );
        }
        let sub = initial_shared(BackdropSource::Substrate);
        assert_ne!(bytemuck::bytes_of(&*sub), bytemuck::bytes_of(&base), "substrate is a look");
        // The azimuth override is applied AFTER the look, and really does replace it — the
        // ordering is the whole content of that constant's doc comment.
        assert_eq!(sub.lighting[4], SUBSTRATE_KEY_AZIMUTH_DEG);
        assert_ne!(
            SUBSTRATE_KEY_AZIMUTH_DEG,
            substrate_scene::SUBSTRATE_KEY_AZIMUTH_DEG,
            "if the leaf ever adopts this camera's azimuth, delete the override rather than \
             keeping two constants that agree"
        );
    }

    /// The new value is documented, and quoted from the constant the parser uses — the same
    /// reason the scrim line is formatted rather than restated. A `substrate` that worked and
    /// was undocumented would be `ORGANON_SHELL_PTY_DEBUG`'s mistake a second time.
    #[test]
    fn the_backdrop_line_documents_the_substrate_value() {
        let h = help_text();
        assert!(h.contains(BACKDROP_SUBSTRATE), "help does not name the substrate source");
        assert!(h.contains("0/unset off"), "help does not say what unset means");
    }

    /// The help text has to name the environment variables, because they ARE the interface —
    /// a help screen that omitted them would be true and useless.
    ///
    /// ⚠️ **This array is an allow-list, so it catches a REMOVAL and not an ADDITION.** A
    /// variable deleted from `help_text` fails here; a brand-new flag that is added to
    /// neither stays invisible. `ORGANON_SHELL_PTY_DEBUG` is how that was found — it
    /// shipped on its own branch, green, while the help it is meant to be documented by
    /// said nothing about it. Add a flag to `help_text` and to this array together.
    #[test]
    fn help_names_every_documented_environment_variable() {
        let h = help_text();
        for var in [
            "ORGANON_SHELL_BACKDROP",
            "ORGANON_SHELL_SCRIM",
            "ORGANON_SHELL_TABS",
            "ORGANON_SHELL_DEFAULT",
            "ORGANON_SHELL_CMD",
            "ORGANON_SHELL_PTY_DEBUG",
            "ORGANON_IPC_NS",
        ] {
            assert!(h.contains(var), "help does not mention {var}");
        }
    }
}
