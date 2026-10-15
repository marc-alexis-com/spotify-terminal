//! The sleep timer, the lyrics, a track's radio, the devices, and changes to
//! the listed playlist: what goes beyond browsing and playing.
use super::{Jukebox, listing::Source};
use crate::{
    config,
    spotify::{Album, Command, Device, Direction, LyricLine},
};
use hypercmd::{Event, EventPayload, Input, Key, Node};
use std::{
    rc::Rc,
    time::{Duration, Instant},
};

/// When the sleep timer pauses the music.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Sleep {
    At(Instant),
    /// When the playing track ends.
    EndOfTrack,
}

/// The lyrics of a track, by its URI.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Lyrics {
    pub(crate) uri: String,
    pub(crate) lines: Rc<[LyricLine]>,
}

/// Lines of the lyrics around the one sung.
const LYRIC_CONTEXT: usize = 7;

/// A line of lyrics, or a note for a line without words.
fn words(line: &LyricLine) -> String {
    if line.words.trim().is_empty() {
        "♪".to_owned()
    } else {
        line.words.clone()
    }
}

impl Jukebox {
    /// Off, then each duration of the settings, then the end of the track, then off.
    pub(super) fn cycle_sleep(&self) {
        let minutes = &config::get().sleep.minutes;
        let next = match self.sleep.get_untracked() {
            None => minutes.first().map_or(Some(Sleep::EndOfTrack), |first| {
                Some(Sleep::At(Instant::now() + Duration::from_secs(first * 60)))
            }),
            Some(Sleep::At(at)) => {
                let left = at
                    .saturating_duration_since(Instant::now())
                    .as_secs()
                    .div_ceil(60);
                // The next duration longer than what is left, or the end of the track.
                minutes
                    .iter()
                    .find(|minutes| **minutes > left)
                    .map_or(Some(Sleep::EndOfTrack), |next| {
                        Some(Sleep::At(Instant::now() + Duration::from_secs(next * 60)))
                    })
            }
            Some(Sleep::EndOfTrack) => None,
        };
        self.sleep.set(next);
        match next {
            None => self.inform("Sleep timer off."),
            Some(Sleep::EndOfTrack) => self.inform("The music stops at the end of this track."),
            Some(Sleep::At(_)) => self.inform(format!("The music stops in {}.", self.sleep_left())),
        }
    }

    /// Pauses once the sleep timer is due; the clock calls it.
    pub(super) fn check_sleep(&self) {
        if let Some(Sleep::At(at)) = self.sleep.get_untracked()
            && Instant::now() >= at
        {
            self.fall_asleep();
        }
    }

    /// Pauses as a track starts, if the timer waited for the end of the one before.
    pub(super) fn sleep_on_track_change(&self) {
        if self.sleep.get_untracked() == Some(Sleep::EndOfTrack) {
            self.fall_asleep();
        }
    }

    fn fall_asleep(&self) {
        self.sleep.set(None);
        self.command(Command::Pause);
        self.inform("Sleep timer: the music stopped.");
    }

    fn sleep_left(&self) -> String {
        match self.sleep.get_untracked() {
            Some(Sleep::At(at)) => {
                let minutes = at
                    .saturating_duration_since(Instant::now())
                    .as_secs()
                    .div_ceil(60);
                format!("{minutes} min")
            }
            Some(Sleep::EndOfTrack) => "end of track".into(),
            None => String::new(),
        }
    }

    /// The timer beside the shuffle and repeat buttons, while it runs.
    pub(super) fn sleep_label(&self) -> String {
        self.clock.get();
        match self.sleep.get() {
            Some(_) => format!("☾ {}", self.sleep_left()),
            None => String::new(),
        }
    }

    pub(super) fn toggle_lyrics(self: &Rc<Self>) {
        if self.lyrics_open.get_untracked() {
            self.lyrics_open.set(false);
            self.focus("tracks");
            return;
        }
        if self.playing_track().is_none() {
            self.inform("Play a track to see its lyrics.");
            return;
        }
        self.lyrics_open.set(true);
        self.load_lyrics();
    }

    /// Loads the playing track's lyrics, unless they are there.
    pub(super) fn load_lyrics(self: &Rc<Self>) {
        let (Some(track), Some(catalog)) = (self.playing_track(), self.catalog()) else {
            return;
        };
        if !self.lyrics_open.get_untracked()
            || self.lyrics.with_untracked(|lyrics| {
                lyrics
                    .as_ref()
                    .is_some_and(|lyrics| lyrics.uri == track.uri)
            })
        {
            return;
        }
        let lyrics = self.lyrics.clone();
        let started = self.spawn(async move {
            let lines = catalog.lyrics(track.uri.clone()).await?;
            lyrics.set(Some(Lyrics {
                uri: track.uri.clone(),
                lines: lines.into(),
            }));
            Ok(())
        });
        if let Err(error) = started {
            self.warn(error.to_string());
        }
    }

    pub(super) fn lyrics_shown(&self) -> bool {
        self.lyrics_open.get()
    }

    pub(super) fn close_lyrics(&self) {
        self.lyrics_open.set(false);
        self.focus("tracks");
    }

    pub(super) fn lyrics_title(&self) -> String {
        self.playing_track()
            .map(|track| format!("{} · {}", track.name, track.artist_names()))
            .unwrap_or_default()
    }

    /// The lines around the one sung now, and which of them it is.
    pub(super) fn lyric_lines(&self) -> Vec<(usize, String, bool)> {
        self.frame.get();
        let uri = self.playing_track().map(|track| track.uri.clone());
        let elapsed = self.elapsed_ms();
        self.lyrics.with(|lyrics| {
            let Some(lyrics) = lyrics
                .as_ref()
                .filter(|lyrics| Some(&lyrics.uri) == uri.as_ref())
            else {
                return vec![(0, "Loading the lyrics…".into(), false)];
            };
            if lyrics.lines.is_empty() {
                return vec![(0, "Spotify has no lyrics for this track.".into(), false)];
            }
            let timed = lyrics.lines.iter().any(|line| line.start_ms > 0);
            let current = timed
                .then(|| {
                    lyrics
                        .lines
                        .iter()
                        .rposition(|line| line.start_ms <= elapsed)
                })
                .flatten();
            let first = current.map_or(0, |current| current.saturating_sub(LYRIC_CONTEXT / 2));
            lyrics
                .lines
                .iter()
                .enumerate()
                .skip(first)
                .take(LYRIC_CONTEXT * 2)
                .map(|(index, line)| (index, words(line), Some(index) == current))
                .collect()
        })
    }

    /// Opens the radio of the selected track, or of what plays.
    pub(super) fn open_radio(self: &Rc<Self>) {
        let track = self.selected().or_else(|| self.playing_track());
        match track {
            Some(track) if track.uri.starts_with("spotify:track:") => {
                self.drill(Source::Radio(Rc::new(Album {
                    uri: track.uri.clone(),
                    name: track.name.clone(),
                })));
                self.focus("tracks");
            }
            Some(_) => self.inform("Spotify has radios of songs, not of episodes."),
            None => self.inform("Select a track to hear its radio."),
        }
    }

    /// Lists the account's devices, to move the playback to one.
    pub(super) fn open_devices(self: &Rc<Self>) {
        let Some(catalog) = self.catalog() else {
            return;
        };
        let devices = self.devices.clone();
        let jukebox = self.clone();
        let started = self.spawn(async move {
            let found = catalog.devices().await?;
            if found.is_empty() {
                jukebox.inform("Spotify lists no device of this account.");
                return Ok(());
            }
            devices.set(Some(found.into_iter().map(Rc::new).collect()));
            Ok(())
        });
        if let Err(error) = started {
            self.warn(error.to_string());
        }
    }

    pub(super) fn devices_shown(&self) -> bool {
        self.devices.with(Option::is_some)
    }

    pub(super) fn device_list(&self) -> Vec<Rc<Device>> {
        self.devices
            .with(|devices| devices.clone().unwrap_or_default())
    }

    pub(super) fn device_label(device: &Device) -> String {
        let mut label = device.name.clone();
        if device.here {
            label.push_str(" (this computer)");
        }
        if device.active {
            label = format!("♫ {label} · active");
        }
        format!("{label}  {}", device.kind)
    }

    pub(super) fn close_devices(&self) {
        self.devices.set(None);
        self.focus("tracks");
    }

    /// Moves the playback to `device`.
    pub(super) fn play_on(self: &Rc<Self>, device: &Device) {
        let Some(catalog) = self.catalog() else {
            return;
        };
        // Whether the music leaves this computer, which must then let it go.
        let leaving = self.devices.with_untracked(|devices| {
            devices
                .iter()
                .flatten()
                .any(|other| other.active && other.here)
        });
        self.close_devices();
        if device.active {
            self.inform(format!("{} is already the device in use.", device.name));
            return;
        }
        let (jukebox, id, name) = (self.clone(), device.id.clone(), device.name.clone());
        let started = self.services.spawn(&self.owner, async move {
            match catalog.transfer(id).await {
                Ok(()) => jukebox.music_moved(&name, leaving),
                Err(error) => jukebox.warn(error.to_string()),
            }
        });
        if let Err(error) = started {
            self.warn(error.to_string());
        }
    }

    /// Says where the music went; if it left this computer, lets it go: see
    /// `let_music_go`.
    fn music_moved(self: &Rc<Self>, name: &str, leaving: bool) {
        if leaving {
            self.let_music_go();
            self.inform(format!(
                "Music moves to {name}; this computer joins again when you play here."
            ));
        } else {
            self.inform(format!("Music moves to {name}."));
        }
    }

    /// Up and down move between the devices; Esc closes.
    pub(super) fn devices_key(&self, event: &Event) {
        self.list_key(event, "device-list", Self::close_devices);
    }

    /// Up and down between the buttons of the list `id`, then back to its
    /// field if it has one; Esc runs `close`.
    pub(super) fn list_key(&self, event: &Event, id: &str, close: fn(&Self)) {
        let EventPayload::Input(Input::Key { key, .. }) = event.payload else {
            return;
        };
        let buttons = self.buttons_of(id);
        let Some(index) = buttons.iter().position(|button| *button == event.target) else {
            return;
        };
        match key {
            Key::Escape => close(self),
            Key::Down | Key::Char('j') => {
                if let Some(next) = buttons.get(index + 1) {
                    next.request_focus();
                }
            }
            Key::Up | Key::Char('k') => match index.checked_sub(1) {
                Some(previous) => buttons[previous].request_focus(),
                None if id == "picker-list" => self.focus("picker-name"),
                None => {}
            },
            _ => return,
        }
        event.prevent_default();
    }

    pub(super) fn buttons_of(&self, id: &str) -> Vec<Node> {
        self.root
            .borrow()
            .as_ref()
            .and_then(|root| root.find(id))
            .map(|list| {
                list.descendants()
                    .filter(|node| node.tag() == "button")
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The listed playlist, when its own order shows, for changes to it.
    fn editable_playlist(&self) -> Option<String> {
        if self.full.borrow().is_some() {
            self.inform("Change a playlist in its own order: S back to it, Esc the filter.");
            return None;
        }
        let playlist = self
            .listing
            .with_untracked(|listing| match &listing.source {
                Source::Playlist(playlist) => Some(playlist.uri.clone()),
                _ => None,
            });
        if playlist.is_none() {
            self.inform("Open one of your playlists to change it.");
        }
        playlist
    }

    /// Takes the selected track out of the listed playlist.
    pub(super) fn remove_from_playlist(self: &Rc<Self>) {
        let (Some(playlist), Some(track), Some(catalog)) =
            (self.editable_playlist(), self.selected(), self.catalog())
        else {
            return;
        };
        let jukebox = self.clone();
        let started = self.services.spawn(&self.owner, async move {
            match catalog
                .remove_from_playlist(&playlist, track.uri.clone())
                .await
            {
                Ok(()) => {
                    jukebox.inform(format!("Took “{}” out of the playlist.", track.name));
                    jukebox.reload_listed(0);
                }
                Err(error) => jukebox.warn(error.to_string()),
            }
        });
        if let Err(error) = started {
            self.warn(error.to_string());
        }
    }

    /// Moves the selected track one place up or down in the listed playlist.
    pub(super) fn move_in_playlist(self: &Rc<Self>, direction: Direction) {
        let (Some(playlist), Some(track), Some(catalog)) =
            (self.editable_playlist(), self.selected(), self.catalog())
        else {
            return;
        };
        let jukebox = self.clone();
        let started = self.services.spawn(&self.owner, async move {
            match catalog
                .move_in_playlist(&playlist, track.uri.clone(), direction)
                .await
            {
                Ok(()) => jukebox.reload_listed(match direction {
                    Direction::Up => -1,
                    Direction::Down => 1,
                }),
                Err(error) => jukebox.warn(error.to_string()),
            }
        });
        if let Err(error) = started {
            self.warn(error.to_string());
        }
    }

    /// Lists the source again, its selection moved by `shift` rows, after a change.
    fn reload_listed(self: &Rc<Self>, shift: isize) {
        let selected = self.window.with_untracked(|window| window.selected);
        self.pending_row
            .set(Some(selected.saturating_add_signed(shift)));
        let source = self
            .listing
            .with_untracked(|listing| listing.source.clone());
        if let Source::Playlist(playlist) = &source {
            self.headers.borrow_mut().remove(&playlist.uri);
        }
        self.open(source);
    }

    /// Asks a new name for the listed playlist.
    pub(super) fn start_rename(&self) {
        let Some(name) =
            self.editable_playlist().and(self.listing.with_untracked(
                |listing| match &listing.source {
                    Source::Playlist(playlist) => Some(playlist.name.clone()),
                    _ => None,
                },
            ))
        else {
            return;
        };
        self.rename_text.set(name);
        self.renaming.set(true);
    }

    pub(super) fn renaming_shown(&self) -> bool {
        self.renaming.get()
    }

    /// Enter renames the playlist; Esc leaves it as it is.
    pub(super) fn rename_key(self: &Rc<Self>, event: &Event) {
        let EventPayload::Input(Input::Key { key, .. }) = event.payload else {
            return;
        };
        match key {
            Key::Escape => self.close_rename(),
            Key::Enter => self.rename(),
            _ => return,
        }
        event.prevent_default();
    }

    pub(super) fn close_rename(&self) {
        self.renaming.set(false);
        self.focus("tracks");
    }

    fn rename(self: &Rc<Self>) {
        let name = self
            .rename_text
            .with_untracked(|text| text.trim().to_owned());
        self.renaming.set(false);
        self.focus("tracks");
        let (Some(playlist), Some(catalog)) = (self.editable_playlist(), self.catalog()) else {
            return;
        };
        if name.is_empty() {
            return;
        }
        let jukebox = self.clone();
        let started = self.services.spawn(&self.owner, async move {
            match catalog.rename_playlist(&playlist, name.clone()).await {
                Ok(()) => {
                    jukebox.inform(format!("Renamed the playlist {name}."));
                    jukebox.load_playlists();
                    let renamed = Source::Playlist(Rc::new(crate::spotify::Playlist {
                        uri: playlist,
                        name,
                    }));
                    jukebox.headers.borrow_mut().clear();
                    jukebox.open(renamed);
                }
                Err(error) => jukebox.warn(error.to_string()),
            }
        });
        if let Err(error) = started {
            self.warn(error.to_string());
        }
    }
}
