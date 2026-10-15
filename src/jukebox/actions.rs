//! What changes the library, and what spt keeps of the playback: likes, the
//! queue, adding to a playlist, and where podcast episodes were left. Spotify
//! lets this client change the library, but not read the queue nor the resume
//! points of the account: spt keeps those itself, for this computer.
use super::{Jukebox, listing::Source, memory};
use crate::{
    config,
    spotify::{self, Playlist, Resume, Track},
};
use hypercmd::{Event, EventPayload, Input, Key};
use std::{collections::HashSet, rc::Rc, time::Duration};

const EPISODE_PREFIX: &str = "spotify:episode:";
/// An episode within this much of its end counts as played.
const PLAYED_MARGIN: Duration = Duration::from_secs(60);
/// How far an episode moves before its position is saved again.
const SAVE_EVERY: Duration = Duration::from_secs(10);

/// The tracks the playlist picker adds, and how it names them.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Picker {
    pub(crate) uris: Vec<String>,
    pub(crate) label: String,
}

impl Jukebox {
    /// Loads which tracks are in Liked Songs, for their hearts.
    pub(super) fn load_saved(&self) {
        let Some(catalog) = self.catalog() else {
            return;
        };
        let (saved, liked_at) = (self.saved.clone(), self.liked_at.clone());
        let started = self.spawn(async move {
            let dates = catalog.liked_dates().await?;
            saved.set(dates.keys().cloned().collect());
            liked_at.replace(dates);
            Ok(())
        });
        if let Err(error) = started {
            self.warn(error.to_string());
        }
    }

    pub(super) fn is_saved(&self, uri: &str) -> bool {
        self.saved.with(|saved| saved.contains(uri))
    }

    /// Likes the selected row, or takes its like back.
    pub(super) fn like_selected(self: &Rc<Self>) {
        match self.selected() {
            Some(track) => self.toggle_like(&track),
            None => self.inform("Select a track to like it."),
        }
    }

    /// Likes what plays, or takes its like back.
    pub(super) fn like_playing(self: &Rc<Self>) {
        match self.playing_track() {
            Some(track) => self.toggle_like(&track),
            None => self.inform("Nothing plays to like."),
        }
    }

    /// Changes the heart at once, and puts it back if Spotify refuses.
    /// A like taken back keeps its date, so that liking the track again puts it
    /// back in its place in Liked Songs rather than at the top.
    fn toggle_like(self: &Rc<Self>, track: &Track) {
        let Some(catalog) = self.catalog() else {
            return;
        };
        let (uri, name) = (track.uri.clone(), track.name.clone());
        let like = !self.saved.with_untracked(|saved| saved.contains(&uri));
        let flip = |saved: &mut HashSet<String>, on: bool, uri: &str| {
            if on {
                saved.insert(uri.to_owned());
            } else {
                saved.remove(uri);
            }
        };
        if !like && let Some(date) = self.liked_at.borrow().get(&uri).copied() {
            // Kept before anything changes, so that no failure can lose it.
            self.unliked.borrow_mut().insert(uri.clone(), date);
            self.save_unliked();
        }
        let first_date = self.unliked.borrow().get(&uri).copied().filter(|_| like);
        self.saved.update(|saved| flip(saved, like, &uri));
        let jukebox = self.clone();
        let started = self.services.spawn(&self.owner, async move {
            let done = if like {
                catalog.save(uri.clone(), first_date).await
            } else {
                catalog.unsave(uri.clone()).await
            };
            match done {
                Ok(()) => jukebox.liked_changed(&uri, &name, like, first_date),
                Err(error) => {
                    jukebox.saved.update(|saved| flip(saved, !like, &uri));
                    jukebox.warn(error.to_string());
                }
            }
        });
        if let Err(error) = started {
            self.warn(error.to_string());
        }
    }

    /// Notes a like given or taken back, and says so.
    fn liked_changed(&self, uri: &str, name: &str, like: bool, first_date: Option<u32>) {
        if !like {
            self.liked_at.borrow_mut().remove(uri);
            self.inform(format!(
                "Took back the like of “{name}”; liking it again puts it back in its place."
            ));
            return;
        }
        let date = first_date.unwrap_or_else(|| {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
            now.map_or(0, |since| {
                u32::try_from(since.as_secs()).unwrap_or(u32::MAX)
            })
        });
        self.liked_at.borrow_mut().insert(uri.to_owned(), date);
        if first_date.is_some() {
            self.unliked.borrow_mut().remove(uri);
            self.save_unliked();
            self.inform(format!(
                "Liked “{name}” again, back in its place of {}.",
                day(date)
            ));
        } else {
            self.inform(format!("Liked “{name}”."));
        }
    }

    fn save_unliked(&self) {
        if let Err(error) = memory::remember_unliked(&self.unliked.borrow()) {
            self.warn(format!("Cannot keep the date of this like: {error}"));
        }
    }

    /// Notes a track queued here, for the queue view.
    pub(super) fn queued(self: &Rc<Self>, uri: String) {
        self.queue.borrow_mut().push(uri);
        self.refresh_queue();
    }

    /// What plays next: the tracks queued here, then the rest of the list playing,
    /// unless shuffle hides its order.
    pub(super) fn up_next(&self) -> Vec<String> {
        let mut next = self.queue.borrow().clone();
        // A queued track plays in between: the list goes on from its own last track.
        let current = self.list_position.borrow().clone();
        let shuffle = self.mode().shuffle;
        if let (Some(current), false) = (current, shuffle) {
            let list = self.playing_list.borrow();
            if let Some(index) = list.iter().position(|uri| *uri == current) {
                next.extend(list[index + 1..].iter().cloned());
            }
        }
        next
    }

    /// Drops the queued tracks up to `uri`, which starts playing; a track that
    /// was not queued moves the list on.
    pub(super) fn dequeue(&self, uri: &str) {
        let mut queue = self.queue.borrow_mut();
        match queue.iter().position(|queued| queued == uri) {
            Some(index) => drop(queue.drain(..=index)),
            None => drop(self.list_position.replace(Some(uri.to_owned()))),
        }
    }

    pub(super) fn show_queue(self: &Rc<Self>) {
        self.open_shelf(Source::Queue);
    }

    /// Reads the queue again if it is listed, as it changes with each track.
    pub(super) fn refresh_queue(self: &Rc<Self>) {
        if self
            .listing
            .with_untracked(|listing| listing.source == Source::Queue)
        {
            let window = self.window.get_untracked();
            self.pending_row.set(Some(window.selected));
            self.open(Source::Queue);
        }
    }

    /// Opens the playlist picker for the selected row.
    pub(super) fn pick_playlist(&self) {
        let Some(track) = self.selected() else {
            self.inform("Select a track to add it to a playlist.");
            return;
        };
        self.picker_text.set(String::new());
        self.picker.set(Some(Rc::new(Picker {
            uris: vec![track.uri.clone()],
            label: track.name.clone(),
        })));
        // The field takes the focus by itself once it shows: see ui/picker.html.
    }

    pub(super) fn close_picker(&self) {
        self.picker.set(None);
        self.focus("tracks");
    }

    pub(super) fn picker_shown(&self) -> bool {
        self.picker.with(Option::is_some)
    }

    pub(super) fn picker_title(&self) -> String {
        self.picker.with(|picker| {
            picker
                .as_ref()
                .map(|picker| format!("Add “{}” to a playlist", picker.label))
                .unwrap_or_default()
        })
    }

    /// The playlists of the library whose name holds the typed text.
    pub(super) fn picker_playlists(&self) -> Vec<Rc<Playlist>> {
        let typed = self.picker_text.with(|text| text.trim().to_lowercase());
        self.library.with(|library| {
            library
                .iter()
                .filter_map(|entry| match &entry.item {
                    spotify::LibraryItem::Playlist(playlist) => Some(playlist.clone()),
                    spotify::LibraryItem::Folder { .. } => None,
                })
                .filter(|playlist| playlist.name.to_lowercase().contains(&typed))
                .collect()
        })
    }

    pub(super) fn picker_hint(&self) -> String {
        let typed = self.picker_text.with(|text| text.trim().to_owned());
        let exact = self
            .picker_playlists()
            .into_iter()
            .find(|playlist| playlist.name.eq_ignore_ascii_case(&typed));
        match exact {
            _ if typed.is_empty() => {
                "Type to find a playlist, or a new name to create one · ↓ chooses · Esc closes"
                    .into()
            }
            Some(playlist) => format!("Enter adds to {} · ↓ chooses another", playlist.name),
            None => format!("Enter creates the playlist “{typed}” · ↓ chooses one found"),
        }
    }

    /// Enter in the picker's field: the first playlist found, or a new one by that name.
    pub(super) fn picker_key(self: &Rc<Self>, event: &Event) {
        let EventPayload::Input(Input::Key { key, .. }) = event.payload else {
            return;
        };
        match key {
            Key::Escape => self.close_picker(),
            Key::Enter => {
                let typed = self
                    .picker_text
                    .with_untracked(|text| text.trim().to_owned());
                let found = self.picker_playlists();
                let exact = found
                    .iter()
                    .find(|playlist| playlist.name.eq_ignore_ascii_case(&typed))
                    .cloned();
                match (exact, found.first().cloned()) {
                    (Some(playlist), _) => self.add_to(&playlist),
                    (None, _) if !typed.is_empty() => self.create_and_add(typed),
                    (None, Some(first)) => self.add_to(&first),
                    (None, None) => return,
                }
            }
            Key::Down => self.focus_first_pick(),
            _ => return,
        }
        event.prevent_default();
    }

    /// Up and down move between the playlists; Esc closes.
    pub(super) fn picker_list_key(&self, event: &Event) {
        let EventPayload::Input(Input::Key { key, .. }) = event.payload else {
            return;
        };
        let buttons = self.picks();
        let Some(index) = buttons.iter().position(|button| *button == event.target) else {
            return;
        };
        match key {
            Key::Escape => self.close_picker(),
            Key::Down | Key::Char('j') => {
                if let Some(next) = buttons.get(index + 1) {
                    next.request_focus();
                }
            }
            Key::Up | Key::Char('k') => match index.checked_sub(1) {
                Some(previous) => buttons[previous].request_focus(),
                None => self.focus("picker-name"),
            },
            _ => return,
        }
        event.prevent_default();
    }

    fn picks(&self) -> Vec<hypercmd::Node> {
        self.root
            .borrow()
            .as_ref()
            .and_then(|root| root.find("picker-list"))
            .map(|list| {
                list.descendants()
                    .filter(|node| node.tag() == "button")
                    .collect()
            })
            .unwrap_or_default()
    }

    fn focus_first_pick(&self) {
        if let Some(first) = self.picks().first() {
            first.request_focus();
        }
    }

    pub(super) fn add_to(self: &Rc<Self>, playlist: &Playlist) {
        let (Some(catalog), Some(picker)) = (self.catalog(), self.picker.get_untracked()) else {
            return;
        };
        self.close_picker();
        let (jukebox, uri, name) = (self.clone(), playlist.uri.clone(), playlist.name.clone());
        let started = self.services.spawn(&self.owner, async move {
            match catalog.add_to_playlist(&uri, picker.uris.clone()).await {
                Ok(()) => jukebox.inform(format!("Added “{}” to {name}.", picker.label)),
                Err(error) => jukebox.warn(error.to_string()),
            }
        });
        if let Err(error) = started {
            self.warn(error.to_string());
        }
    }

    fn create_and_add(self: &Rc<Self>, name: String) {
        let (Some(catalog), Some(picker)) = (self.catalog(), self.picker.get_untracked()) else {
            return;
        };
        self.close_picker();
        let jukebox = self.clone();
        let started = self.services.spawn(&self.owner, async move {
            let added = async {
                let uri = catalog.create_playlist(name.clone()).await?;
                catalog.add_to_playlist(&uri, picker.uris.clone()).await
            };
            match added.await {
                Ok(()) => {
                    jukebox.inform(format!("Created {name} with “{}”.", picker.label));
                    jukebox.load_playlists();
                }
                Err(error) => jukebox.warn(error.to_string()),
            }
        });
        if let Err(error) = started {
            self.warn(error.to_string());
        }
    }

    /// Where `uri` should start: where it was left, for an episode and if the settings say so.
    pub(super) fn resume_position(&self, uri: &str) -> u32 {
        if !uri.starts_with(EPISODE_PREFIX) || !config::get().podcasts.resume {
            return 0;
        }
        self.resume.with_untracked(|resume| {
            resume
                .get(uri)
                .filter(|point| !point.fully_played)
                .map_or(0, |point| u32::try_from(point.position_ms).unwrap_or(0))
        })
    }

    /// The mark of an episode's row: played, begun, or none.
    pub(super) fn episode_mark(&self, uri: &str) -> Option<&'static str> {
        if !uri.starts_with(EPISODE_PREFIX) || !config::get().podcasts.mark_played {
            return None;
        }
        self.resume.with(|resume| resume.get(uri).and_then(mark))
    }

    /// Keeps where the playing episode is, so that it starts there next time; the
    /// clock calls it, and it writes every few seconds of listening at most.
    pub(super) fn remember_episode(&self, uri: &str, elapsed: Duration, length: Duration) {
        if !uri.starts_with(EPISODE_PREFIX) || !config::get().podcasts.resume {
            return;
        }
        let point = Resume {
            position_ms: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
            fully_played: !length.is_zero() && elapsed + PLAYED_MARGIN >= length,
        };
        let moved = self.resume.with_untracked(|resume| {
            resume.get(uri).is_none_or(|known| {
                known.fully_played != point.fully_played
                    || known.position_ms.abs_diff(point.position_ms)
                        >= u64::try_from(SAVE_EVERY.as_millis()).unwrap_or(u64::MAX)
            })
        });
        if !moved {
            return;
        }
        self.resume.update(|resume| {
            resume.insert(uri.to_owned(), point);
        });
        let positions = self.resume.with_untracked(Clone::clone);
        if let Err(error) = memory::remember_episodes(&positions) {
            self.warn(format!("Cannot save where this episode is: {error}"));
        }
    }
}

/// Played, begun, or neither.
fn mark(point: &Resume) -> Option<&'static str> {
    if point.fully_played {
        Some("✓")
    } else if point.position_ms > 0 {
        Some("◐")
    } else {
        None
    }
}

/// The day of a date in seconds since 1970, as `2022-06-09`.
fn day(seconds: u32) -> String {
    // Howard Hinnant's days-to-civil, for the proleptic Gregorian calendar.
    let days = i64::from(seconds) / 86_400 + 719_468;
    let era = days.div_euclid(146_097);
    let of_era = days.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * of_year + 2) / 153;
    let day = of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates_read_as_days() {
        assert_eq!(super::day(1_654_795_474), "2022-06-09");
        assert_eq!(super::day(0), "1970-01-01");
        assert_eq!(super::day(951_782_400), "2000-02-29");
    }
}
