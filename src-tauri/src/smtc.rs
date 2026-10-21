//! Contrôles multimédia de Windows (SMTC) : panneau du volume, écran verrouillé,
//! touches multimédia, avec la pochette et le titre en cours.
//! Les objets Windows vivent sur le fil principal (fil de la fenêtre).

#[cfg(windows)]
mod imp {
    use std::cell::RefCell;
    use std::ffi::c_void;
    use std::time::Duration;

    use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig};

    thread_local! {
        static CTRL: RefCell<Option<MediaControls>> = const { RefCell::new(None) };
    }

    /// Commandes reçues de Windows.
    pub enum Event {
        Toggle,
        Play,
        Pause,
        Next,
        Prev,
        Stop,
        Seek(f64),
    }

    pub fn init(hwnd: *mut c_void, on_event: impl Fn(Event) + Send + 'static) -> bool {
        let cfg = PlatformConfig { dbus_name: "fr.clem.music", display_name: "MusicPi", hwnd: Some(hwnd) };
        let Ok(mut c) = MediaControls::new(cfg) else { return false };
        let ok = c
            .attach(move |e: MediaControlEvent| {
                let ev = match e {
                    MediaControlEvent::Toggle => Event::Toggle,
                    MediaControlEvent::Play => Event::Play,
                    MediaControlEvent::Pause => Event::Pause,
                    MediaControlEvent::Next => Event::Next,
                    MediaControlEvent::Previous => Event::Prev,
                    MediaControlEvent::Stop => Event::Stop,
                    MediaControlEvent::SetPosition(MediaPosition(d)) => Event::Seek(d.as_secs_f64()),
                    _ => return,
                };
                on_event(ev);
            })
            .is_ok();
        if ok {
            CTRL.with(|x| *x.borrow_mut() = Some(c));
        }
        ok
    }

    pub fn update(title: &str, artist: &str, album: &str, cover: Option<&str>, duration: f64, playing: bool, position: f64) {
        CTRL.with(|x| {
            if let Some(c) = x.borrow_mut().as_mut() {
                let _ = c.set_metadata(MediaMetadata {
                    title: Some(title),
                    artist: Some(artist),
                    album: Some(album),
                    cover_url: cover,
                    duration: (duration > 0.0).then(|| Duration::from_secs_f64(duration)),
                });
                let progress = Some(MediaPosition(Duration::from_secs_f64(position.max(0.0))));
                let _ = c.set_playback(if playing { MediaPlayback::Playing { progress } } else { MediaPlayback::Paused { progress } });
            }
        });
    }

    pub fn stopped() {
        CTRL.with(|x| {
            if let Some(c) = x.borrow_mut().as_mut() {
                let _ = c.set_playback(MediaPlayback::Stopped);
            }
        });
    }
}

#[cfg(windows)]
pub use imp::*;

#[cfg(not(windows))]
mod imp_stub {
    pub enum Event {
        Toggle,
        Play,
        Pause,
        Next,
        Prev,
        Stop,
        Seek(f64),
    }
    pub fn init(_hwnd: *mut std::ffi::c_void, _on_event: impl Fn(Event) + Send + 'static) -> bool {
        false
    }
    pub fn update(_t: &str, _a: &str, _al: &str, _c: Option<&str>, _d: f64, _p: bool, _pos: f64) {}
    pub fn stopped() {}
}

#[cfg(not(windows))]
pub use imp_stub::*;
