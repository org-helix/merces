//! The window without the system frame (title bar and border are drawn by the
//! app, see `ui/titlebar.rs`).
//!
//! - Windows and Linux: no decorations at all. The app's top bar moves the
//!   window (system drag, so Windows Aero snap still works), a double-click
//!   maximises or restores it, and thin invisible edges resize it. On Windows
//!   11 the corners stay rounded and the 1 px border takes the theme's
//!   hairline colour (DWM); no shadow.
//! - macOS: the content extends under a transparent title bar; the native
//!   traffic lights stay at their usual place on the left, native resizing and
//!   rounded corners are kept, without the title and without shadow.

use bevy::ecs::system::NonSendMarker;
use bevy::math::CompassOctant;
use bevy::prelude::*;
use bevy::window::{PresentMode, PrimaryWindow, WindowResized, WindowResolution};

pub const MACOS: bool = cfg!(target_os = "macos");

/// The primary window, without the system frame.
pub fn primary() -> Window {
    Window {
        title: "Merces".into(),
        resolution: WindowResolution::new(1440, 900),
        present_mode: PresentMode::AutoVsync,
        decorations: MACOS,
        titlebar_transparent: true,
        titlebar_show_title: false,
        fullsize_content_view: true,
        has_shadow: false,
        ..default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowRequest {
    Drag,
    Resize(CompassOctant),
    Minimize,
    ToggleMaximize,
    Close,
}

/// Requests from the UI, applied to the window by [`apply`].
#[derive(Resource, Default)]
pub struct WindowCtl {
    pub maximized: bool,
    requests: Vec<WindowRequest>,
}

impl WindowCtl {
    pub fn request(&mut self, r: WindowRequest) {
        self.requests.push(r);
    }
}

pub struct FramelessPlugin;

impl Plugin for FramelessPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WindowCtl>()
            .add_systems(PostUpdate, (apply, track_maximized).chain());
        #[cfg(target_os = "windows")]
        app.add_systems(Update, native_style);
    }
}

fn apply(
    mut ctl: ResMut<WindowCtl>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
) {
    if ctl.requests.is_empty() {
        return;
    }
    let Ok(mut w) = windows.single_mut() else {
        return;
    };
    let maximized = ctl.maximized;
    for r in ctl.requests.drain(..) {
        match r {
            WindowRequest::Drag => w.start_drag_move(),
            WindowRequest::Resize(dir) => w.start_drag_resize(dir),
            WindowRequest::Minimize => w.set_minimized(true),
            WindowRequest::ToggleMaximize => w.set_maximized(!maximized),
            WindowRequest::Close => {
                exit.write(AppExit::Success);
            }
        }
    }
}

/// Bevy does not report the maximised state: ask winit when the size changes.
fn track_maximized(
    _main_thread: NonSendMarker,
    mut resized: MessageReader<WindowResized>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut ctl: ResMut<WindowCtl>,
    mut first: Local<bool>,
) {
    let changed = resized.read().count() > 0;
    if !changed && *first {
        return;
    }
    let Ok(entity) = windows.single() else {
        return;
    };
    let max = bevy::winit::WINIT_WINDOWS
        .with_borrow(|ws| ws.get_window(entity).map(|w| w.is_maximized()));
    if let Some(max) = max {
        *first = true;
        if ctl.maximized != max {
            ctl.maximized = max;
        }
    }
}

/// Windows 11: rounded corners and a hairline border in the theme's colour.
/// Runs until the native window exists, then once more never.
#[cfg(target_os = "windows")]
fn native_style(
    _main_thread: NonSendMarker,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut done: Local<bool>,
) {
    use winit::platform::windows::{Color, CornerPreference, WindowExtWindows};
    if *done {
        return;
    }
    let Ok(entity) = windows.single() else {
        return;
    };
    bevy::winit::WINIT_WINDOWS.with_borrow(|ws| {
        if let Some(w) = ws.get_window(entity) {
            w.set_corner_preference(CornerPreference::Round);
            w.set_undecorated_shadow(false);
            let h = crate::theme::DARK.tile;
            w.set_border_color(Some(Color::from_rgb(h.r(), h.g(), h.b())));
            *done = true;
        }
    });
}
