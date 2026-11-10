//! Cad's desktop shell as a library: the frame logic, services, native menu, workspace file,
//! control server and input devices around [`iridium_cad_ui_egui::CadApp`]. The app binary hosts
//! it (and keeps what belongs to the process: logging, GPU backend, updater, exit).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod control_server;
mod desktop_theme;
#[cfg(target_os = "macos")]
mod native_menu;
mod spacemouse;
mod system_animations;
mod workspace;

use iridium_cad_engine::Session;
use iridium_cad_ui_egui::{CadApp, Services, i18n};
use std::path::{Path, PathBuf};

/// Stencil bits of the window's render pass; the host sets the same in `NativeOptions::stencil_buffer`.
const STENCIL_BITS: u8 = 8;

/// What the app was started with.
#[derive(Debug, Clone, Default)]
pub struct Launch {
    /// `--control <port>`: start the localhost control server.
    pub control: Option<u16>,
    /// `--sample`: open with the sample drawing.
    pub sample: bool,
    /// Files to open.
    pub files: Vec<PathBuf>,
    /// The settings directory (`IRIDIUM_CAD_CONFIG_DIR`); `None`: no settings folder.
    pub config_dir: Option<PathBuf>,
    /// `IRIDIUM_CAD_SYSTEM_CURSOR`: use the system cursor by default.
    pub system_cursor: bool,
    /// `IRIDIUM_CAD_LOCALE`: the language, instead of the system's.
    pub locale: Option<String>,
    /// `IRIDIUM_CAD_NO_NATIVE_MENU`: the menus stay inside the window.
    pub no_native_menu: bool,
    /// The surface presents without vsync: cap the frame rate (see `gpu::FrameCap`).
    pub frame_cap: bool,
    /// Font name for missing text styles (`IRIDIUM_CAD_FONT_ALT`).
    pub font_alt: Option<String>,
    /// Fallback font list (`IRIDIUM_CAD_FONT_FALLBACK`).
    pub font_fallback: Option<String>,
}

/// The Cad shell: one per window.
pub struct Shell {
    cad: CadApp,
    spacemouse: spacemouse::SpaceMouse,
    /// The desktop's light/dark choice where winit reports none (Linux, the BSDs).
    desktop_theme: desktop_theme::DesktopTheme,
    /// Whether the OS shows animations, for the System choice of View ▸ Accessibility ▸ Animation Effects.
    system_animations: system_animations::SystemAnimations,
    workspace: workspace::Workspace,
    workspace_error: Option<String>,
    #[cfg(target_os = "macos")]
    native_menu: Option<native_menu::NativeMenu>,
    #[cfg(target_os = "macos")]
    menus_in_window: bool,
    /// The `movable` flag last set on the main window.
    #[cfg(target_os = "macos")]
    movable: Option<bool>,
    /// Set by [`Shell::install_native_menu`]: `logic` rebuilds the menu.
    #[cfg(target_os = "macos")]
    reinstall_menu: std::sync::atomic::AtomicBool,
    fonts: egui::FontDefinitions,
    style: egui::Style,
    zoom: f32,
}

impl Shell {
    /// The standalone app's constructor: the host-free [`Shell::new_for_host`] plus whatever needs the
    /// real window. Cad has no such extra besides the wgpu render state, which a host sets itself.
    pub fn new(cc: &eframe::CreationContext<'_>, launch: Launch) -> Self {
        let mut shell = Self::new_for_host(&cc.egui_ctx, cc.storage, launch);
        if let Some(rs) = &cc.wgpu_render_state {
            shell.cad.set_wgpu(rs, eframe::egui_wgpu::depth_format_from_bits(0, STENCIL_BITS));
        }
        shell
    }

    /// Host-facing constructor: no `CreationContext`, so a host can build this module later. `ctx`
    /// is the host's egui context, `storage` its persistent store (`None`: defaults, nothing read).
    /// Installs the I/O hooks and settings, restores the saved state, opens `launch.files`, then
    /// lets the app install its look into `ctx` and captures it (see [`Shell::fonts`]).
    pub fn new_for_host(ctx: &egui::Context, storage: Option<&dyn eframe::Storage>, launch: Launch) -> Self {
        install_io();
        iridium_cad_engine::settings_dir::set(launch.config_dir.clone());
        if let Some(dir) = &launch.config_dir {
            iridium_cad_ui_egui::fault::install_report_hook(dir.join(iridium_cad_ui_egui::fault::REPORT_DIR));
        }
        if let Some(name) = &launch.font_alt {
            iridium_cad_engine::fonts::ttf::set_font_alt(name);
        }
        if let Some(list) = &launch.font_fallback {
            iridium_cad_engine::fonts::ttf::set_fallback_fonts(list);
        }
        let mut app = CadApp::new(Session::empty(), services());
        let (workspace, restored, workspace_error) = workspace::Workspace::load(launch.config_dir.as_deref());
        if let Some(ui) = restored {
            app.ui = ui;
        }
        app.set_system_cursor_default(launch.system_cursor);
        if let Some(error) = &workspace_error {
            eprintln!("Cad workspace: {error}");
            app.set_status(error);
        }
        app.system_languages = launch.locale.map(|s| vec![s]).unwrap_or_else(|| sys_locale::get_locales().collect());
        // The user profile (Options): drafting settings, selection sizes, drawing-area colours.
        if let Some(profile) = storage.and_then(|s| s.get_string(iridium_cad_ui_egui::options::PROFILE_KEY)) {
            app.load_profile(&profile);
        }
        if let Some(prefs) = storage.and_then(|s| s.get_string(iridium_cad_ui_egui::PREFS_KEY)) {
            app.load_prefs(&prefs);
        }
        if let Some(prefs) = storage.and_then(|s| s.get_string(iridium_cad_ui_egui::autosave::PREFS_KEY)) {
            iridium_cad_ui_egui::autosave::load_prefs(&prefs);
        }
        // Autosave files (SAVEFILEPATH) default to `<settings dir>/autosave`.
        if let Some(dir) = &launch.config_dir {
            app.enable_autosave(&dir.join("autosave"));
            app.enable_update_check();
            // Startup LISP (acad.lsp, acaddoc.lsp) is read from `<settings>/support`; the
            // folder is made so it is there to put them in.
            let support = dir.join("support");
            if let Err(e) = std::fs::create_dir_all(&support) {
                log::warn!("no LISP support folder {}: {e}", support.display());
            }
            app.session.set_lisp_support_dir(Some(support));
        }
        app.integrated_titlebar = cfg!(target_os = "macos");
        if launch.frame_cap {
            app.frame_cap = Some(iridium_cad_ui_egui::gpu::FrameCap::new(240));
        }
        if let Some(port) = launch.control {
            let rx = control_server::start(port, ctx.clone());
            app = app.with_control(rx);
        }
        if launch.sample {
            let _ = app.run("ui.sample", serde_json::json!({}));
        }
        for f in &launch.files {
            app.open_path(f);
        }
        if app.session.docs.is_empty() {
            // In the units the profile asks for (MEASUREINIT, restored above).
            let metric = app.session.settings.measureinit;
            app.session.new_drawing(metric);
        }
        let repaint = ctx.clone();
        let desktop_theme = desktop_theme::DesktopTheme::start(move || repaint.request_repaint());
        let repaint = ctx.clone();
        let system_animations = system_animations::SystemAnimations::start(move || repaint.request_repaint());
        app.install_look(ctx);
        let fonts = app.installed_fonts.clone().unwrap_or_default();
        Shell {
            desktop_theme,
            system_animations,
            cad: app,
            spacemouse: spacemouse::SpaceMouse::start(ctx.clone()),
            workspace,
            workspace_error,
            #[cfg(target_os = "macos")]
            native_menu: None,
            #[cfg(target_os = "macos")]
            menus_in_window: launch.no_native_menu,
            #[cfg(target_os = "macos")]
            movable: None,
            #[cfg(target_os = "macos")]
            reinstall_menu: std::sync::atomic::AtomicBool::new(false),
            fonts,
            style: ctx.global_style().as_ref().clone(),
            zoom: ctx.zoom_factor(),
        }
    }

    /// The font set this module installed, captured at construction (stale if fonts change later).
    pub fn fonts(&self) -> egui::FontDefinitions {
        self.fonts.clone()
    }

    /// The style this module left behind, captured at construction.
    pub fn style(&self) -> egui::Style {
        self.style.clone()
    }

    /// The zoom factor this module left behind, captured at construction.
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// macOS: have the next [`Shell::logic`] re-install this module's native menu. No-op elsewhere.
    pub fn install_native_menu(&self) {
        #[cfg(target_os = "macos")]
        self.reinstall_menu.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Per-frame logic, before [`Shell::ui`]: syncs the OS animation setting and runs `CadApp::logic`;
    /// on pass 0 (the first pass or a hidden tick) it also installs/polls the macOS native menu,
    /// polls the spacemouse and persists the workspace; a quit request closes the viewport.
    pub fn logic(&mut self, ctx: &egui::Context) {
        self.cad.system_animations = self.system_animations.current();
        // Consume control/open/screenshot queues and time-based autosave, including hidden ticks.
        self.cad.guarded_logic(ctx);
        // Host frame work runs here, once per frame, and so also on a hidden logic-only tick (which
        // never reaches `ui`): native menus stay current, the spacemouse still navigates and a
        // workspace changed while hidden is persisted. `current_pass_index()` is zero on the first
        // pass and on every hidden tick, and non-zero on later passes of a multi-pass frame.
        if ctx.current_pass_index() == 0 {
            #[cfg(target_os = "macos")]
            {
                if self.reinstall_menu.swap(false, std::sync::atomic::Ordering::Relaxed) {
                    self.native_menu = None;
                }
                if self.native_menu.is_none() && !self.menus_in_window {
                    self.native_menu = Some(native_menu::NativeMenu::install(&mut self.cad, ctx));
                }
                if let Some(m) = &mut self.native_menu {
                    m.poll(&mut self.cad, ctx);
                }
            }
            self.spacemouse.poll(&mut self.cad, ctx);
            // After `cad.logic`, so a control command's workspace change is saved in the same frame.
            self.persist_workspace(Some(ctx));
        }
        if std::mem::take(&mut self.cad.quit_requested) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Apply the desktop-theme fallback, forward raw input to `CadApp`, and capture Tab at the command line.
    pub fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        follow_desktop(raw, self.desktop_theme.current());
        self.cad.raw_input_hook(ctx, raw);
        iridium_cad_ui_egui::cmdline::capture_tab(ctx, raw);
    }

    /// Draw the app into `ui`.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.cad.guarded_ui(ui);
        #[cfg(target_os = "macos")]
        craft_window::title_bar::sync_movable(ui.ctx(), &mut self.movable);
    }

    /// Open a file by path.
    pub fn open_path(&mut self, path: &str) {
        self.cad.open_path(Path::new(path));
    }

    /// A normal exit: the workspace is saved, and this run's autosave files are not needed (a
    /// crash leaves them for recovery).
    pub fn save_state(&mut self) {
        self.persist_workspace(None);
        self.cad.cancel_screenshots();
        self.cad.shutdown_autosave();
    }

    /// Preferences (the interface theme) survive restarts; eframe saves the window state itself
    /// (`persist_window`); egui state is not kept. Called from `eframe::App::save`.
    pub fn save_prefs(&self, storage: &mut dyn eframe::Storage) {
        storage.set_string(iridium_cad_ui_egui::options::PROFILE_KEY, self.cad.profile_json());
        storage.set_string(iridium_cad_ui_egui::PREFS_KEY, self.cad.prefs_json());
        storage.set_string(iridium_cad_ui_egui::autosave::PREFS_KEY, iridium_cad_ui_egui::autosave::prefs_json());
    }

    /// False with unsaved changes or a file still being read: the host asks before closing.
    pub fn can_close(&self) -> bool {
        !self.cad.opening.busy() && !self.cad.session.docs.iter().any(|d| d.is_dirty())
    }

    /// `ctx` is `None` only for the final attempt at exit.
    fn persist_workspace(&mut self, ctx: Option<&egui::Context>) {
        let result = match ctx {
            None => self.workspace.save_on_exit(&self.cad.ui),
            Some(ctx) => self.workspace.save_if_changed(&self.cad.ui, std::time::Instant::now()).map(|wait| {
                if let Some(wait) = wait {
                    ctx.request_repaint_after(wait);
                }
            }),
        };
        if let Err(error) = result {
            if self.workspace_error.as_ref() != Some(&error) {
                eprintln!("Cad workspace: {error}");
                self.cad.set_status(format!("Workspace could not be saved: {error}"));
                self.workspace_error = Some(error);
            }
        } else {
            self.workspace_error = None;
        }
    }
}

/// View ▸ Theme ▸ System resolves against `raw.system_theme`, which winit leaves empty on Linux
/// and the BSDs: the desktop's answer, where it gives one, stands in for it.
fn follow_desktop(raw: &mut egui::RawInput, desktop: Option<egui::Theme>) {
    if desktop.is_some() {
        raw.system_theme = desktop;
    }
}

/// File format hooks for the engine (it stays I/O-agnostic).
fn install_io() {
    iridium_cad_engine::cmd::file::set_io(iridium_cad_engine::cmd::file::IoHooks {
        read: |b, name| iridium_cad_io::read(b, name).map_err(|e| e.to_string()),
        write: |d, name| iridium_cad_io::write(d, name).map_err(|e| e.to_string()),
        plot: Some(|d, space, opts| iridium_cad_io::plotter::plot(d, space, opts).map_err(|e| e.to_string())),
    });
    iridium_cad_engine::cmd::file::set_write_as(|d, name, o| {
        iridium_cad_io::write_with(d, name, &iridium_cad_io::SaveOptions { version: o.version, binary: o.binary }).map_err(|e| e.to_string())
    });
    // AutoLISP's entget, entmake and entmod see entities as DXF groups.
    iridium_cad_engine::cmd::file::set_entity_codec(iridium_cad_engine::cmd::file::EntityCodec {
        groups: iridium_cad_io::entity_groups,
        entity: iridium_cad_io::entity_from_groups,
    });
}

fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter(i18n::t("Drawings (DWG, DXF)"), &["dwg", "dxf", "dwt"])
                .add_filter(i18n::t("SVG drawings (SVG, SVGZ)"), &["svg", "svgz"])
                .add_filter(i18n::t("All files"), &["*"])
                .pick_file()
                .map(|p| p.to_string_lossy().to_string())
        })),
        pick_save: Some(Box::new(|name: &str| {
            filters_for_save(name)
                .iter()
                .fold(rfd::FileDialog::new().set_file_name(name), |d, (label, ext)| d.add_filter(i18n::t(label), &[*ext]))
                .save_file()
                .map(|p| p.to_string_lossy().to_string())
        })),
        // APPLOAD: AutoLISP routines only, starting where the last one was loaded from.
        pick_lisp: Some(Box::new(|dir: &str| {
            let dialog = rfd::FileDialog::new().add_filter(i18n::t("AutoLISP (LSP)"), &["lsp"]).add_filter(i18n::t("All files"), &["*"]);
            let dialog = if dir.is_empty() { dialog } else { dialog.set_directory(dir) };
            dialog.pick_file().map(|p| p.to_string_lossy().to_string())
        })),
        no_file_system: false,
    }
}

/// File types for the Save dialog, the suggested name's type first: Windows adds the first
/// filter's extension, and macOS accepts only listed extensions, replacing any other with the
/// first one. DWG must be listed for a `.dwg` name to survive, and a drawing opened from DWG
/// then defaults to DWG; other names keep DXF first.
fn filters_for_save(name: &str) -> [(&'static str, &'static str); 6] {
    let mut filters =
        [("Drawing (DXF)", "dxf"), ("Drawing (DWG)", "dwg"), ("Drawing Template (DWT)", "dwt"), ("SVG", "svg"), ("PNG", "png"), ("PDF", "pdf")];
    let lower = name.to_ascii_lowercase();
    filters.sort_by_key(|(_, ext)| !lower.ends_with(&format!(".{ext}")));
    filters
}

#[cfg(test)]
mod tests {
    use super::{filters_for_save, follow_desktop};

    #[test]
    fn save_dialog_offers_dwg_with_the_drawing_format_first() {
        let first = |name: &str| filters_for_save(name)[0].1;
        assert_eq!(first("Waco Winnelson Floorplan.dwg"), "dwg");
        assert_eq!(first("FLOORPLAN.DWG"), "dwg");
        // New drawings and DXF files keep DXF first.
        assert_eq!(first("Drawing1.dxf"), "dxf");
        assert_eq!(first("Drawing1"), "dxf");
        assert_eq!(first("plan.pdf"), "pdf");
        assert_eq!(first("office.dwt"), "dwt");
        for name in ["a.dwg", "b.dxf", "c"] {
            let exts: Vec<_> = filters_for_save(name).iter().map(|(_, e)| *e).collect();
            assert!(exts.contains(&"dwg") && exts.contains(&"dxf"), "{name}");
        }
    }

    /// View ▸ Theme ▸ System on Linux: winit reports no theme, so the desktop's answer stands in;
    /// without one (or where winit answers and the desktop doesn't) the input is left alone.
    #[test]
    fn the_desktop_answers_for_the_system_theme() {
        let mut raw = egui::RawInput::default();
        follow_desktop(&mut raw, Some(egui::Theme::Light));
        assert_eq!(raw.system_theme, Some(egui::Theme::Light));
        let mut raw = egui::RawInput { system_theme: Some(egui::Theme::Dark), ..Default::default() };
        follow_desktop(&mut raw, None);
        assert_eq!(raw.system_theme, Some(egui::Theme::Dark));
        let mut raw = egui::RawInput::default();
        follow_desktop(&mut raw, None);
        assert_eq!(raw.system_theme, None);
    }
}
