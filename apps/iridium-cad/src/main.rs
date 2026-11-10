//! Cad desktop app.
//!
//! Usage: `iridium_cad [--control <port>] [--sample] [files…]` (`iridium_cad --help` lists the rest; see [`launch_args`])
//!
//! `--control <port>` (or `IRIDIUM_CAD_CONTROL_PORT`) starts a localhost JSON-lines control server:
//! `{"id":1,"method":"cmdline.input","params":{"text":"circle 0,0 5"}}` → `{"id":1,"ok":true,…}`.
//! See `iridium_cad_ui_egui::control` for the methods.
//!
//! `IRIDIUM_CAD_VSYNC=1` presents with vsync on Linux/BSD, where the default is the low-latency
//! present mode (`IRIDIUM_CAD_VSYNC=0` selects that elsewhere); see `iridium_cad_ui_egui::gpu::surface_config`.
//!
//! `log` records go to standard error and `<settings dir>/logs/iridium_cad.log` (`RUST_LOG` sets the
//! levels); see [`logging`]. The `IRIDIUM_CAD_*` variables are read once, at startup; see [`env`].
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod env;
#[cfg(any(target_os = "windows", test))]
mod graphics;
mod launch_args;
mod logging;

use iridium_cad_shell::{Launch, Shell};
use iridium_cad_ui_egui::switch::Switch;

struct App {
    shell: Shell,
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.shell.logic(ctx);
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.shell.raw_input_hook(ctx, raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.shell.ui(ui);
    }
    fn on_exit(&mut self) {
        self.shell.save_state();
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.shell.save_prefs(storage);
    }
    fn persist_egui_memory(&self) -> bool {
        false
    }
}

/// Whether eframe's storage file (`app.ron`, a map of keys to strings) holds a saved main window
/// state. An unreadable or malformed file counts as none, as it does for eframe.
fn saved_window(app_ron: &str) -> bool {
    ron::from_str::<std::collections::HashMap<String, String>>(app_ron).is_ok_and(|kv| kv.contains_key(WINDOW_KEY))
}

/// The window state from the last session is saved and will be restored (#2595).
fn restores_window() -> bool {
    eframe::storage_dir(APP_ID).and_then(|dir| std::fs::read_to_string(dir.join("app.ron")).ok()).is_some_and(|ron| saved_window(&ron))
}

/// The main window: its size, title bar, placement and rendering needs. `restoring`: a window state
/// saved by the last session is restored instead of opening centred.
fn window_options(restoring: bool) -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Cad")
            .with_inner_size([1600.0, 1000.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_app_id(APP_ID),
        // The size, position and maximized state are kept between sessions (#2595); eframe keeps a
        // restored window on the monitors that are connected now.
        persist_window: true,
        // The first launch opens centred on its screen, not cascaded from the top-left corner
        // (#1129); later launches go back where the window was (eframe would centre over that).
        // Wayland compositors place windows themselves.
        centered: !restoring,
        // Cad installs its own capped font chain (`theme::install_fonts`); never run
        // system-font discovery as a hidden fallback.
        system_font_fallback: false,
        // A stencil buffer clips the contents of clipped layout viewports on the GPU.
        stencil_buffer: STENCIL_BITS,
        ..Default::default()
    }
}

const APP_ID: &str = "ph.cad";

/// The key eframe saves the main window state under in its storage.
const WINDOW_KEY: &str = "window";

/// Stencil bits of the window's render pass (see `NativeOptions::stencil_buffer`).
const STENCIL_BITS: u8 = 8;

#[cfg(target_os = "macos")]
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/app-icon/iridium-cad-macos-512.png");
#[cfg(not(target_os = "macos"))]
const APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/app-icon/iridium-cad-256.png");

fn app_icon() -> Option<egui::IconData> {
    eframe::icon_data::from_png_bytes(APP_ICON_PNG).map_err(|e| log::warn!("app icon: {e}")).ok()
}

fn version_string() -> String {
    format!("iridium_cad {}", iridium_cad_engine::build_version())
}

/// Parse CLI options (`--control`, `--sample`, files, `--version`), install logging and I/O hooks, build the services and app, and run eframe.
fn main() -> eframe::Result {
    // A copy of this program started to read or write a DWG file does only that.
    if let Some(code) = iridium_cad_io::dwg_worker::serve_if_asked() {
        std::process::exit(code);
    }
    iridium_cad_io::dwg_worker::use_own_worker();
    // First, so every start-up record is captured; see `logging`.
    let logger = logging::install();
    // The update helper relaunches this binary after a self-update; it runs and exits here.
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(done) = craft_update::self_update::finish(&iridium_cad_ui_egui::UPDATE_APP, &std::env::args_os().skip(1).collect::<Vec<_>>()) {
        if let Err(e) = done {
            eprintln!("Cad update helper failed: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    // `--help` and `--version` print to standard output and exit 0. A usage error (an unknown or
    // repeated flag, a missing or bad value, a bad IRIDIUM_CAD_CONTROL_PORT) names the argument on
    // standard error and exits 2 before the window opens.
    let launch = launch_args::parse(launch_args::command(), std::env::args_os().skip(1)).unwrap_or_else(|e| e.exit());
    let vars = env::Env::read();
    // The log file lives under the settings directory; opened after the arguments, so `--version`
    // leaves no file behind. Records logged until now are written to it first.
    if let Some(logger) = logger {
        match vars.config_dir.as_ref().map(|d| logger.attach_dir(&d.join(logging::LOG_DIR))) {
            Some(Ok(path)) => log::info!("{} ({} {}), log file {}", version_string(), std::env::consts::OS, std::env::consts::ARCH, path.display()),
            Some(Err(e)) => log::warn!("no log file: {e}"),
            None => {
                logger.stderr_only();
                log::warn!("no log file: no settings directory (an empty IRIDIUM_CAD_CONFIG_DIR turns it off)");
            }
        }
    }
    let surface = iridium_cad_ui_egui::gpu::surface_config(vars.vsync, cfg!(all(unix, not(target_os = "macos"))));
    let mut options = eframe::NativeOptions {
        // The crosshair is drawn by the app: present without a queue on Linux/BSD, where X11
        // swapchains hold two frames back under vsync (`IRIDIUM_CAD_VSYNC=1` restores vsync).
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration::default().with_surface_config(surface),
        ..window_options(restores_window())
    };
    if let Some(icon) = app_icon() {
        options.viewport = options.viewport.with_icon(icon);
    }
    // Before eframe creates the wgpu instance: default Windows to DirectX 12 with FXC (see graphics.rs).
    #[cfg(target_os = "windows")]
    graphics::configure(&mut options, eframe::wgpu::Backends::from_env(), eframe::wgpu::Dx12Compiler::from_env());
    // Command previews (COPY, MOVE...) of large selections are tessellated by egui, whose index
    // buffer outgrew wgpu's default 256 MiB cap; the validation error is fatal. Ask for what the
    // adapter actually supports.
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup {
        let base = std::sync::Arc::clone(&create.device_descriptor);
        create.device_descriptor = std::sync::Arc::new(move |adapter| {
            let mut d = base(adapter);
            d.required_limits.max_buffer_size = d.required_limits.max_buffer_size.max(adapter.limits().max_buffer_size);
            d
        });
    }
    // winit has no file drag-and-drop on Wayland (only on X11), so dropping files from the file
    // manager showed a "no" cursor. Run through XWayland when it's there; IRIDIUM_CAD_WAYLAND=1
    // keeps the native Wayland backend.
    #[cfg(all(unix, not(target_os = "macos")))]
    if std::env::var_os("DISPLAY").is_some() && vars.wayland != Switch::On {
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    // macOS: Quit Cad (Cmd+Q), the Dock's Quit and logout end the app through AppKit's
    // terminate, which would skip the unsaved-changes prompt (#2157). winit turns each into a
    // close request for the window instead, so it asks first, as the close button does.
    #[cfg(target_os = "macos")]
    {
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            b.with_close_requested_on_terminate(true);
        }));
    }
    let launch = Launch {
        control: launch.control,
        sample: launch.sample,
        files: launch.files,
        config_dir: vars.config_dir,
        system_cursor: vars.system_cursor == Switch::On,
        locale: vars.locale,
        no_native_menu: vars.no_native_menu == Switch::On,
        frame_cap: surface.present_mode == eframe::wgpu::PresentMode::AutoNoVsync,
        font_alt: vars.font_alt,
        font_fallback: vars.font_fallback,
    };
    eframe::run_native("Cad", options, Box::new(move |cc| Ok(Box::new(App { shell: Shell::new(cc, launch) }))))
}

#[cfg(test)]
mod tests {
    use super::{saved_window, window_options};

    #[test]
    fn the_first_launch_opens_centred() {
        let o = window_options(false);
        assert!(o.centered, "centred on launch");
        assert!(o.persist_window, "the window state is saved for the next launch");
    }

    #[test]
    fn a_saved_window_reopens_where_it_was() {
        let o = window_options(true);
        assert!(!o.centered, "centring would move a restored window");
        assert!(o.persist_window);
    }

    #[test]
    fn a_saved_window_is_read_from_app_ron() {
        assert!(saved_window(r#"{"iridium_cad.prefs": "{}", "window": "(maximized: true)"}"#));
        assert!(!saved_window(r#"{"iridium_cad.prefs": "{\"window\":1}"}"#), "a value mentioning it is not the key");
        assert!(!saved_window(""));
        assert!(!saved_window("{\"window\": "), "truncated");
        assert!(!saved_window("not ron at all"));
    }
}
