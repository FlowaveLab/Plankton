use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};

use serde::{Deserialize, Serialize};
#[cfg(target_os = "windows")]
use tauri::Theme;
use tauri::{
    image::Image,
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Emitter, Manager, Runtime, State,
};
use tracing::error;

use crate::background::{request_explicit_quit, show_main_window};

const TRAY_ID: &str = "plankton-status";
const TRAY_NAVIGATE_EVENT: &str = "plankton://navigate";
const ANIMATION_INTERVAL: std::time::Duration = std::time::Duration::from_millis(160);
const REASONING_FRAME_COUNT: u8 = 8;

#[derive(Debug)]
pub struct TrayRuntime {
    activity: Mutex<TrayActivity>,
    generation: AtomicU64,
    reduced_motion: AtomicBool,
}

impl Default for TrayRuntime {
    fn default() -> Self {
        Self {
            activity: Mutex::new(TrayActivity::Idle),
            generation: AtomicU64::new(0),
            reduced_motion: AtomicBool::new(false),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayActivity {
    Idle,
    Attention,
    Reasoning,
    Degraded,
    Disconnected,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TraySignals {
    pub(crate) disconnected: bool,
    pub(crate) attention: bool,
    pub(crate) degraded: bool,
    pub(crate) reasoning: bool,
}

pub(crate) fn select_activity(signals: TraySignals) -> TrayActivity {
    if signals.disconnected {
        TrayActivity::Disconnected
    } else if signals.attention {
        TrayActivity::Attention
    } else if signals.degraded {
        TrayActivity::Degraded
    } else if signals.reasoning {
        TrayActivity::Reasoning
    } else {
        TrayActivity::Idle
    }
}

impl TrayActivity {
    fn tooltip(self) -> &'static str {
        match self {
            Self::Idle => "Plankton is ready",
            Self::Attention => "Plankton needs your approval",
            Self::Reasoning => "Plankton is evaluating a request",
            Self::Degraded => "Plankton needs attention",
            Self::Disconnected => "Plankton daemon is disconnected",
        }
    }

    fn animates(self, reduced_motion: bool) -> bool {
        self == Self::Reasoning && !reduced_motion
    }
}

pub fn setup(app: &mut App) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .text("open", "Open Plankton")
        .text("requests", "Requests")
        .text("passwords", "Passwords")
        .text("diagnostics", "Diagnostics")
        .separator()
        .text("quit", "Quit Plankton")
        .build()?;
    let image = base_image(app.handle())?;

    TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .icon(image)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip(TrayActivity::Idle.tooltip())
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => report(show_main_window(app), "open main window"),
            "requests" => navigate(app, "requests"),
            "passwords" => navigate(app, "passwords"),
            "diagnostics" => navigate(app, "diagnostics"),
            "quit" => request_explicit_quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                report(show_main_window(tray.app_handle()), "open main window");
            }
        })
        .build(app)?;

    Ok(())
}

#[tauri::command]
pub async fn set_tray_activity(
    activity: TrayActivity,
    app: AppHandle,
    _state: State<'_, TrayRuntime>,
) -> Result<(), String> {
    update_activity(&app, activity).await
}

pub async fn update_activity<R: Runtime>(
    app: &AppHandle<R>,
    activity: TrayActivity,
) -> Result<(), String> {
    let state = app
        .try_state::<TrayRuntime>()
        .ok_or_else(|| "tray runtime is unavailable".to_string())?;
    let changed = {
        let mut current = state
            .activity
            .lock()
            .map_err(|_| "failed to lock tray activity".to_string())?;
        if *current == activity {
            false
        } else {
            *current = activity;
            true
        }
    };
    if !changed {
        return Ok(());
    }
    let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let reduced_motion = state.reduced_motion.load(Ordering::SeqCst);
    update_icon(app, activity, 0)?;

    if activity.animates(reduced_motion) {
        spawn_reasoning_animation(app.clone(), generation);
    }
    Ok(())
}

fn spawn_reasoning_animation<R: Runtime>(app: AppHandle<R>, generation: u64) {
    tauri::async_runtime::spawn(async move {
        let mut frame = 0_u8;
        loop {
            tokio::time::sleep(ANIMATION_INTERVAL).await;
            let Some(state) = app.try_state::<TrayRuntime>() else {
                return;
            };
            if state.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let Ok(current) = state.activity.lock() else {
                error!("failed to lock tray activity during animation");
                return;
            };
            if !current.animates(state.reduced_motion.load(Ordering::SeqCst)) {
                return;
            }
            drop(current);
            frame = (frame + 1) % REASONING_FRAME_COUNT;
            if let Err(message) = update_icon(&app, TrayActivity::Reasoning, frame) {
                error!(%message, "failed to animate tray reasoning state");
                return;
            }
        }
    });
}

fn transition_reduced_motion(
    state: &TrayRuntime,
    reduced_motion: bool,
) -> Result<(TrayActivity, Option<u64>), String> {
    let activity = *state
        .activity
        .lock()
        .map_err(|_| "failed to lock tray activity".to_string())?;
    state.reduced_motion.store(reduced_motion, Ordering::SeqCst);
    let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    Ok((
        activity,
        activity.animates(reduced_motion).then_some(generation),
    ))
}

#[tauri::command]
pub async fn set_tray_reduced_motion(
    reduced_motion: bool,
    app: AppHandle,
    state: State<'_, TrayRuntime>,
) -> Result<(), String> {
    let (activity, animation_generation) = transition_reduced_motion(&state, reduced_motion)?;
    update_icon(&app, activity, 0)?;
    if let Some(generation) = animation_generation {
        spawn_reasoning_animation(app, generation);
    }
    Ok(())
}

fn navigate<R: Runtime>(app: &AppHandle<R>, page: &str) {
    if let Err(error) =
        show_main_window(app).and_then(|()| app.emit_to("main", TRAY_NAVIGATE_EVENT, page))
    {
        error!(page, %error, "failed to navigate from tray");
    }
}

fn report(result: tauri::Result<()>, operation: &'static str) {
    if let Err(error) = result {
        error!(operation, %error, "tray operation failed");
    }
}

fn update_icon<R: Runtime>(
    app: &AppHandle<R>,
    activity: TrayActivity,
    frame: u8,
) -> Result<(), String> {
    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| "Plankton tray icon is unavailable".to_string())?;
    tray.set_tooltip(Some(activity.tooltip()))
        .map_err(|error| format!("failed to update tray tooltip: {error}"))?;
    let base = base_image(app).map_err(|error| format!("failed to decode tray icon: {error}"))?;
    let rendered = if activity == TrayActivity::Reasoning {
        let spinner = reasoning_frame_image(app, frame)
            .map_err(|error| format!("failed to decode tray spinner: {error}"))?;
        render_activity_icon(&base, &spinner, activity)
    } else {
        render_activity_icon(&base, &base, activity)
    };
    tray.set_icon(Some(rendered))
        .map_err(|error| format!("failed to update tray icon: {error}"))
}

fn base_image<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Image<'static>> {
    #[cfg(target_os = "windows")]
    let bytes = if match app.get_webview_window("main") {
        Some(window) => window.theme()? == Theme::Light,
        None => false,
    } {
        include_bytes!("../assets/tray/generated/windows/plankton-tray-light-32.png").as_slice()
    } else {
        include_bytes!("../assets/tray/generated/windows/plankton-tray-dark-32.png").as_slice()
    };
    #[cfg(not(target_os = "windows"))]
    let bytes = {
        let _ = app;
        include_bytes!("../assets/tray/generated/macos/plankton-trayTemplate@2x.png").as_slice()
    };
    Image::from_bytes(bytes).map(Image::to_owned)
}

fn reasoning_frame_image<R: Runtime>(
    app: &AppHandle<R>,
    frame: u8,
) -> tauri::Result<Image<'static>> {
    #[cfg(target_os = "windows")]
    let light_shell = match app.get_webview_window("main") {
        Some(window) => window.theme()? == Theme::Light,
        None => false,
    };
    #[cfg(target_os = "windows")]
    let bytes = if light_shell {
        windows_spinner_frame_bytes(true, frame)
    } else {
        windows_spinner_frame_bytes(false, frame)
    };
    #[cfg(not(target_os = "windows"))]
    let bytes = {
        let _ = app;
        macos_spinner_frame_bytes(frame)
    };
    Image::from_bytes(bytes).map(Image::to_owned)
}

#[cfg(target_os = "windows")]
fn windows_spinner_frame_bytes(light_shell: bool, frame: u8) -> &'static [u8] {
    match (light_shell, frame % REASONING_FRAME_COUNT) {
        (true, 0) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-light-32-0.png")
        }
        (true, 1) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-light-32-1.png")
        }
        (true, 2) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-light-32-2.png")
        }
        (true, 3) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-light-32-3.png")
        }
        (true, 4) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-light-32-4.png")
        }
        (true, 5) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-light-32-5.png")
        }
        (true, 6) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-light-32-6.png")
        }
        (true, 7) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-light-32-7.png")
        }
        (false, 0) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-dark-32-0.png")
        }
        (false, 1) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-dark-32-1.png")
        }
        (false, 2) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-dark-32-2.png")
        }
        (false, 3) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-dark-32-3.png")
        }
        (false, 4) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-dark-32-4.png")
        }
        (false, 5) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-dark-32-5.png")
        }
        (false, 6) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-dark-32-6.png")
        }
        (false, 7) => {
            include_bytes!("../assets/tray/generated/windows/plankton-tray-spinner-dark-32-7.png")
        }
        _ => unreachable!("frame is modulo the spinner frame count"),
    }
}

#[cfg(not(target_os = "windows"))]
fn macos_spinner_frame_bytes(frame: u8) -> &'static [u8] {
    match frame % REASONING_FRAME_COUNT {
        0 => {
            include_bytes!("../assets/tray/generated/macos/plankton-tray-spinnerTemplate-0@2x.png")
        }
        1 => {
            include_bytes!("../assets/tray/generated/macos/plankton-tray-spinnerTemplate-1@2x.png")
        }
        2 => {
            include_bytes!("../assets/tray/generated/macos/plankton-tray-spinnerTemplate-2@2x.png")
        }
        3 => {
            include_bytes!("../assets/tray/generated/macos/plankton-tray-spinnerTemplate-3@2x.png")
        }
        4 => {
            include_bytes!("../assets/tray/generated/macos/plankton-tray-spinnerTemplate-4@2x.png")
        }
        5 => {
            include_bytes!("../assets/tray/generated/macos/plankton-tray-spinnerTemplate-5@2x.png")
        }
        6 => {
            include_bytes!("../assets/tray/generated/macos/plankton-tray-spinnerTemplate-6@2x.png")
        }
        7 => {
            include_bytes!("../assets/tray/generated/macos/plankton-tray-spinnerTemplate-7@2x.png")
        }
        _ => unreachable!("frame is modulo the spinner frame count"),
    }
}

fn render_activity_icon(
    base: &Image<'_>,
    reasoning_frame: &Image<'_>,
    activity: TrayActivity,
) -> Image<'static> {
    let mut pixels = if activity == TrayActivity::Reasoning {
        reasoning_frame.rgba().to_vec()
    } else {
        base.rgba().to_vec()
    };
    match activity {
        TrayActivity::Idle | TrayActivity::Reasoning => {}
        TrayActivity::Attention => draw_badge(&mut pixels, base.width(), base.height()),
        TrayActivity::Degraded => draw_slash(&mut pixels, base.width(), base.height()),
        TrayActivity::Disconnected => {
            for alpha in pixels.iter_mut().skip(3).step_by(4) {
                *alpha = ((*alpha as u16 * 2) / 5) as u8;
            }
        }
    }
    Image::new_owned(pixels, base.width(), base.height())
}

fn draw_badge(rgba: &mut [u8], width: u32, height: u32) {
    let size = width.min(height) as i32;
    let radius = (size / 7).max(2);
    let center = (size - radius - 1, radius);
    for y in 0..size {
        for x in 0..size {
            if (x - center.0).pow(2) + (y - center.1).pow(2) <= radius.pow(2) {
                let offset = ((y * width as i32 + x) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[220, 38, 38, 255]);
            }
        }
    }
}

fn draw_slash(rgba: &mut [u8], width: u32, height: u32) {
    let size = width.min(height) as i32;
    for y in 0..size {
        for x in 0..size {
            if (x - y).abs() <= 1 {
                let offset = ((y * width as i32 + x) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[245, 158, 11, 255]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        render_activity_icon, select_activity, transition_reduced_motion, TrayActivity,
        TrayRuntime, TraySignals,
    };
    use tauri::image::Image;

    fn quarter_rotated(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
        assert_eq!(width, height, "test pixels must be square");
        let size = width as usize;
        let mut rotated = vec![0; rgba.len()];
        for y in 0..size {
            for x in 0..size {
                let source = (y * size + x) * 4;
                let destination = (x * size + (size - 1 - y)) * 4;
                rotated[destination..destination + 4].copy_from_slice(&rgba[source..source + 4]);
            }
        }
        rotated
    }

    #[test]
    fn reasoning_only_animates_when_motion_is_allowed() {
        assert!(TrayActivity::Reasoning.animates(false));
        assert!(!TrayActivity::Reasoning.animates(true));
        assert!(!TrayActivity::Idle.animates(false));
    }

    #[test]
    fn tray_activity_uses_action_first_priority() {
        let cases = [
            (
                TraySignals {
                    disconnected: true,
                    attention: true,
                    degraded: true,
                    reasoning: true,
                },
                TrayActivity::Disconnected,
            ),
            (
                TraySignals {
                    attention: true,
                    degraded: true,
                    reasoning: true,
                    ..TraySignals::default()
                },
                TrayActivity::Attention,
            ),
            (
                TraySignals {
                    degraded: true,
                    reasoning: true,
                    ..TraySignals::default()
                },
                TrayActivity::Degraded,
            ),
            (
                TraySignals {
                    reasoning: true,
                    ..TraySignals::default()
                },
                TrayActivity::Reasoning,
            ),
            (TraySignals::default(), TrayActivity::Idle),
        ];

        for (signals, expected) in cases {
            assert_eq!(select_activity(signals), expected);
        }
    }

    #[test]
    fn leaving_reduced_motion_restarts_active_reasoning_animation() {
        let runtime = TrayRuntime::default();
        *runtime.activity.lock().expect("tray activity") = TrayActivity::Reasoning;

        assert_eq!(
            transition_reduced_motion(&runtime, true).expect("enable reduced motion"),
            (TrayActivity::Reasoning, None)
        );
        assert_eq!(
            transition_reduced_motion(&runtime, false).expect("disable reduced motion"),
            (TrayActivity::Reasoning, Some(2))
        );
    }

    #[test]
    fn idle_keeps_directional_brand_pixels_static() {
        let brand = Image::new(
            &[1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255],
            2,
            2,
        );
        let spinner = Image::new(
            &[9, 0, 0, 255, 9, 0, 0, 255, 9, 0, 0, 255, 9, 0, 0, 255],
            2,
            2,
        );

        let idle = render_activity_icon(&brand, &spinner, TrayActivity::Idle);

        assert_eq!(idle.rgba(), brand.rgba());
    }

    #[test]
    fn reasoning_must_not_rotate_directional_brand_pixels() {
        let brand = Image::new(
            &[1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255],
            2,
            2,
        );

        let spinner = Image::new(
            &[9, 0, 0, 255, 9, 0, 0, 255, 9, 0, 0, 255, 9, 0, 0, 255],
            2,
            2,
        );
        let reasoning = render_activity_icon(&brand, &spinner, TrayActivity::Reasoning);
        let rotated_brand = quarter_rotated(brand.rgba(), brand.width(), brand.height());

        assert_eq!(reasoning.rgba(), spinner.rgba());
        assert_ne!(
            reasoning.rgba(),
            rotated_brand.as_slice(),
            "Reasoning must render a dedicated spinner frame, never a rotated brand mark"
        );
    }

    #[test]
    fn disconnected_icon_is_visibly_dimmed() {
        let base = Image::new(&[10, 20, 30, 255], 1, 1);
        let disconnected = render_activity_icon(&base, &base, TrayActivity::Disconnected);
        assert_eq!(disconnected.rgba(), &[10, 20, 30, 102]);
    }
}
