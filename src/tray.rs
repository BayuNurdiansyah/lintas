//! Optional system tray icon (`--tray`), showing the current connection
//! status and offering a Quit action.
//!
//! ksni (StatusNotifierItem over D-Bus) is async-only, so it runs on its own
//! background thread with a small dedicated tokio runtime; the rest of the
//! app stays synchronous. [`TrayHandle::set_status`] bridges that gap: it's
//! a plain blocking call the main serve/host loop can make whenever status
//! changes, with no async anywhere else in the codebase.

use ksni::TrayMethods;
use std::sync::LazyLock;

static ICON: LazyLock<ksni::Icon> = LazyLock::new(|| {
    let img = image::load_from_memory_with_format(
        include_bytes!("../assets/icons/lintas-64.png"),
        image::ImageFormat::Png,
    )
    .expect("bundled icon is a valid PNG");
    let width = img.width() as i32;
    let height = img.height() as i32;
    let mut data = img.into_rgba8().into_vec();
    let (chunks, _) = data.as_chunks_mut::<4>();
    for pixel in chunks {
        pixel.rotate_right(1); // RGBA -> ARGB, which is what the tray spec wants
    }
    ksni::Icon {
        width,
        height,
        data,
    }
});

struct AppTray {
    status: String,
}

impl ksni::Tray for AppTray {
    fn id(&self) -> String {
        "lintas".into()
    }

    fn title(&self) -> String {
        format!("lintas — {}", self.status)
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![ICON.clone()]
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem {
                label: self.status.clone(),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                // The kernel releases any grabbed input devices when the
                // process exits, so this is safe even while devices are held.
                activate: Box::new(|_: &mut Self| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// A running tray icon. Cloning is cheap; every clone controls the same icon.
#[derive(Clone)]
pub struct TrayHandle {
    rt: tokio::runtime::Handle,
    tray: ksni::Handle<AppTray>,
}

impl TrayHandle {
    /// Update the status line shown in the tray's tooltip and menu.
    pub fn set_status(&self, status: impl Into<String>) {
        let status = status.into();
        self.rt.block_on(self.tray.update(|t| t.status = status));
    }
}

/// Starts the tray icon on a background thread. Returns `None` (logging why)
/// if the desktop has no StatusNotifierItem host to show it in — this is
/// optional UI, so that's never a reason to fail the whole program.
pub fn spawn(initial_status: &str) -> Option<TrayHandle> {
    let (tx, rx) = std::sync::mpsc::channel();
    let initial_status = initial_status.to_string();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("Tray icon unavailable: {e}");
                let _ = tx.send(None);
                return;
            }
        };
        rt.block_on(async {
            let tray = AppTray {
                status: initial_status,
            };
            match tray.spawn().await {
                Ok(handle) => {
                    let _ = tx.send(Some(TrayHandle {
                        rt: tokio::runtime::Handle::current(),
                        tray: handle,
                    }));
                }
                Err(e) => {
                    eprintln!("Tray icon unavailable: {e}");
                    let _ = tx.send(None);
                    return;
                }
            }
            // Keep this thread (and its runtime) alive for as long as the
            // tray should exist, i.e. for the lifetime of the process.
            std::future::pending::<()>().await;
        });
    });
    rx.recv().ok().flatten()
}
