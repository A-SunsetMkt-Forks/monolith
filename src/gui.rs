use std::fs;
use std::io::Write;
use std::thread;

use druid::commands;
use druid::widget::{Button, Checkbox, Flex, Label, TextBox};
use druid::{
    AppDelegate, AppLauncher, Command, DelegateCtx, Env, FileDialogOptions, Handled,
    LocalizedString, PlatformError, Target, Widget, WidgetExt, WindowDesc,
};
use druid::{Data, Lens};
use tempfile::{Builder, NamedTempFile};

use monolith::cache::Cache;
use monolith::core::{create_monolithic_document, format_output_path, MonolithError, Options};

const CACHE_ASSET_FILE_SIZE_THRESHOLD: usize = 1024 * 20; // Minimum file size for on-disk caching (in bytes)

struct Delegate;

#[derive(Clone, Data, Lens)]
struct AppState {
    target: String,
    keep_fonts: bool,
    keep_frames: bool,
    keep_images: bool,
    keep_scripts: bool,
    keep_styles: bool,
    output_path: String,
    isolate: bool,
    unwrap_noscript: bool,
    busy: bool,
}

const MONOLITH_GUI_WRITE_OUTPUT: druid::Selector<(Vec<u8>, Option<String>)> =
    druid::Selector::new("monolith-gui.write-output");
const MONOLITH_GUI_ERROR: druid::Selector<MonolithError> =
    druid::Selector::new("monolith-gui.error");

fn main() -> Result<(), PlatformError> {
    let mut program_name: String = env!("CARGO_PKG_NAME").to_string();
    if let Some(l) = program_name.get_mut(0..1) {
        l.make_ascii_uppercase();
    }
    let main_window = WindowDesc::new(ui_builder())
        .title(program_name)
        .with_min_size((640., 320.));
    let state = AppState {
        target: "".to_string(),
        keep_fonts: true,
        keep_frames: true,
        keep_images: true,
        keep_scripts: true,
        keep_styles: true,
        output_path: "".to_string(), // TODO: set it to ~/Downloads/%title%.html by default
        isolate: false,
        unwrap_noscript: false,
        busy: false,
    };

    AppLauncher::with_window(main_window)
        .delegate(Delegate)
        .launch(state)
}

fn ui_builder() -> impl Widget<AppState> {
    let target_input = TextBox::new()
        .with_placeholder("Target (http:/https:/file:/data: URL) or filesystem path")
        .lens(AppState::target)
        .disabled_if(|state: &AppState, _env| state.busy);
    let target_button = Button::new(LocalizedString::new("Open file"))
        .on_click(|ctx, _, _| {
            ctx.submit_command(commands::SHOW_OPEN_PANEL.with(FileDialogOptions::new()))
        })
        .disabled_if(|state: &AppState, _env| state.busy);
    let output_path_label: Label<AppState> =
        Label::new(|state: &AppState, _env: &_| format!("Output path: {}", state.output_path));
    let output_path_button = Button::new(LocalizedString::new("Browse"))
        .on_click(|ctx, _, _| {
            ctx.submit_command(
                commands::SHOW_SAVE_PANEL
                    .with(FileDialogOptions::new().default_name("%title% - %timestamp%.html")),
            )
        })
        .disabled_if(|state: &AppState, _env| state.busy);
    let fonts_checkbox = Checkbox::new("Fonts")
        .lens(AppState::keep_fonts)
        .disabled_if(|state: &AppState, _env| state.busy)
        .padding(5.0);
    let frames_checkbox = Checkbox::new("Frames")
        .lens(AppState::keep_frames)
        .disabled_if(|state: &AppState, _env| state.busy)
        .padding(5.0);
    let images_checkbox = Checkbox::new("Images")
        .lens(AppState::keep_images)
        .disabled_if(|state: &AppState, _env| state.busy)
        .padding(5.0);
    let styles_checkbox = Checkbox::new("Styles")
        .lens(AppState::keep_styles)
        .disabled_if(|state: &AppState, _env| state.busy)
        .padding(5.0);
    let scripts_checkbox = Checkbox::new("Scripts")
        .lens(AppState::keep_scripts)
        .disabled_if(|state: &AppState, _env| state.busy)
        .padding(5.0);
    let isolate_checkbox = Checkbox::new("Isolate")
        .lens(AppState::isolate)
        .disabled_if(|state: &AppState, _env| state.busy)
        .padding(5.0);
    let unwrap_noscript_checkbox = Checkbox::new("Unwrap NOSCRIPTs")
        .lens(AppState::unwrap_noscript)
        .disabled_if(|state: &AppState, _env| state.busy)
        .padding(5.0);
    let start_stop_button = Button::new(LocalizedString::new("Start"))
        .on_click(|ctx, state: &mut AppState, _env| {
            if state.busy {
                return;
            }

            let mut options: Options = Options::default();
            options.ignore_errors = true;
            options.insecure = true;
            options.silent = true;
            options.no_frames = !state.keep_frames;
            options.no_fonts = !state.keep_fonts;
            options.no_images = !state.keep_images;
            options.no_css = !state.keep_styles;
            options.no_js = !state.keep_scripts;
            options.isolate = state.isolate;
            options.unwrap_noscript = state.unwrap_noscript;

            let handle = ctx.get_external_handle();
            let thread_state = state.clone();

            state.busy = true;

            // Set up cache (attempt to create temporary file)
            let temp_cache_file: Option<NamedTempFile> =
                match Builder::new().prefix("monolith-").tempfile() {
                    Ok(tempfile) => Some(tempfile),
                    Err(_) => None,
                };
            let mut cache = Some(Cache::new(
                CACHE_ASSET_FILE_SIZE_THRESHOLD,
                if temp_cache_file.is_some() {
                    Some(
                        temp_cache_file
                            .as_ref()
                            .unwrap()
                            .path()
                            .display()
                            .to_string(),
                    )
                } else {
                    None
                },
            ));

            thread::spawn(move || {
                match create_monolithic_document(thread_state.target, &mut options, &mut cache) {
                    Ok(result) => {
                        handle
                            .submit_command(MONOLITH_GUI_WRITE_OUTPUT, result, Target::Auto)
                            .unwrap();

                        cache.unwrap().destroy_database_file();
                    }
                    Err(error) => {
                        handle
                            .submit_command(MONOLITH_GUI_ERROR, error, Target::Auto)
                            .unwrap();

                        cache.unwrap().destroy_database_file();
                    }
                }
            });
        })
        .disabled_if(|state: &AppState, _env| {
            state.busy || state.target.is_empty() || state.output_path.is_empty()
        });

    Flex::column()
        .with_child(
            Flex::row()
                .with_child(target_input)
                .with_child(target_button),
        )
        .with_child(fonts_checkbox)
        .with_child(frames_checkbox)
        .with_child(images_checkbox)
        .with_child(scripts_checkbox)
        .with_child(styles_checkbox)
        .with_child(
            Flex::row()
                .with_child(output_path_label)
                .with_child(output_path_button),
        )
        .with_child(
            Flex::row()
                .with_child(isolate_checkbox)
                .with_child(unwrap_noscript_checkbox),
        )
        .with_child(start_stop_button)
}

impl AppDelegate<AppState> for Delegate {
    fn command(
        &mut self,
        _ctx: &mut DelegateCtx,
        _target: Target,
        cmd: &Command,
        state: &mut AppState,
        _env: &Env,
    ) -> Handled {
        if let Some(result) = cmd.get(MONOLITH_GUI_WRITE_OUTPUT) {
            let (html, title) = result;

            if !state.output_path.is_empty() {
                match fs::File::create(format_output_path(
                    &state.output_path,
                    &title.clone().unwrap_or_default(),
                )) {
                    Ok(mut file) => {
                        let _ = file.write(&html);
                    }
                    Err(_) => {
                        eprintln!("Error: could not write output");
                    }
                }
            } else {
                eprintln!("Error: no output specified");
            }

            state.busy = false;
            return Handled::Yes;
        }

        if let Some(_error) = cmd.get(MONOLITH_GUI_ERROR) {
            state.busy = false;
            return Handled::Yes;
        }

        if let Some(file_info) = cmd.get(commands::OPEN_FILE) {
            state.target = file_info.path().display().to_string();

            return Handled::Yes;
        }

        if let Some(file_info) = cmd.get(commands::SAVE_FILE_AS) {
            state.output_path = file_info.path().display().to_string();

            return Handled::Yes;
        }

        Handled::No
    }
}
