//! Sorting and filtering the listed tracks. Both need the whole list, so the
//! rest of it loads first; the list as Spotify gives it is kept aside, and
//! what shows is made from it each time the order or the filter changes.
use super::{
    Jukebox,
    listing::{Listing, Loading, Source},
};
use crate::spotify::Track;
use hypercmd::{Event, EventPayload, Input, Key};
use std::rc::Rc;

/// The orders the sort key cycles through.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Sort {
    /// As Spotify gives it.
    #[default]
    Listed,
    Title,
    Artist,
    Album,
    Length,
    /// Last liked first, for Liked Songs.
    Added,
}

impl Sort {
    fn next(self, dated: bool) -> Self {
        match self {
            Self::Listed => Self::Title,
            Self::Title => Self::Artist,
            Self::Artist => Self::Album,
            Self::Album => Self::Length,
            Self::Length if dated => Self::Added,
            Self::Length | Self::Added => Self::Listed,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Listed => "",
            Self::Title => "by title",
            Self::Artist => "by artist",
            Self::Album => "by album",
            Self::Length => "by length",
            Self::Added => "first liked first",
        }
    }
}

impl Jukebox {
    /// The next order of the list.
    pub(super) fn cycle_sort(self: &Rc<Self>) {
        let dated = self
            .listing
            .with_untracked(|listing| listing.source == Source::Liked);
        let sort = self.sort.get_untracked().next(dated);
        self.sort.set(sort);
        match sort.label() {
            "" => self.inform("In Spotify's order."),
            label => self.inform(format!("Sorted {label}.")),
        }
        self.arrange();
    }

    /// Shows the filter field, which narrows the list as you type.
    pub(super) fn start_filter(self: &Rc<Self>) {
        self.filtering.set(true);
        self.focus("filter");
        self.arrange();
    }

    pub(super) fn filter_typed(self: &Rc<Self>) {
        self.arrange();
    }

    /// Enter keeps the filter and goes to the list; Esc drops it.
    pub(super) fn filter_key(self: &Rc<Self>, event: &Event) {
        let EventPayload::Input(Input::Key { key, .. }) = event.payload else {
            return;
        };
        match key {
            Key::Enter | Key::Down => self.focus("tracks"),
            Key::Escape => {
                self.filter.set(String::new());
                self.filtering.set(false);
                self.arrange();
                self.focus("tracks");
            }
            _ => return,
        }
        event.prevent_default();
    }

    pub(super) fn filter_shown(&self) -> bool {
        self.filtering.get()
    }

    /// Forgets the order and the filter, as another list opens.
    pub(super) fn reset_arrangement(&self) {
        self.full.replace(None);
        self.load_all.set(false);
        self.sort.set(Sort::Listed);
        self.filter.set(String::new());
        self.filtering.set(false);
    }

    fn arranged(&self) -> bool {
        self.sort.get_untracked() != Sort::Listed
            || self.filter.with_untracked(|text| !text.trim().is_empty())
    }

    /// Shows the list in the chosen order and filter, once it is whole.
    pub(super) fn arrange(self: &Rc<Self>) {
        if self.full.borrow().is_none() {
            if !self.arranged() {
                return;
            }
            let whole = self.listing.with_untracked(|listing| {
                listing.loading == Loading::Idle && listing.requested >= listing.uris.len()
            });
            if !whole {
                // The rest of the list loads, then this runs again: see `load_more`.
                self.load_all.set(true);
                self.load_more();
                return;
            }
            let listing = self.listing.get_untracked();
            self.full.replace(Some(listing));
        }
        let Some(full) = self.full.borrow().clone() else {
            return;
        };
        let shown = self.arrangement_of(&full);
        self.listing.set(shown);
        self.window.update(|window| window.select(0, 0));
        if !self.arranged() {
            // Back to Spotify's order: nothing is kept aside any more.
            self.full.replace(None);
        }
    }

    /// `full` in the chosen order, narrowed to what the filter names.
    fn arrangement_of(&self, full: &Listing) -> Listing {
        let filter = self
            .filter
            .with_untracked(|text| super::library_search::fold(text.trim()));
        let mut tracks: Vec<Rc<Track>> = full
            .tracks
            .iter()
            .filter(|track| filter.is_empty() || matches(track, &filter))
            .cloned()
            .collect();
        let dates = self.liked_at.borrow();
        let date = |track: &Rc<Track>| dates.get(&track.uri).copied().unwrap_or(0);
        match self.sort.get_untracked() {
            Sort::Listed => {}
            Sort::Title => {
                tracks.sort_by_cached_key(|track| super::library_search::fold(&track.name))
            }
            Sort::Artist => tracks.sort_by_cached_key(|track| {
                (
                    super::library_search::fold(&track.artist_names()),
                    super::library_search::fold(&track.album.name),
                )
            }),
            Sort::Album => {
                tracks.sort_by_cached_key(|track| super::library_search::fold(&track.album.name))
            }
            Sort::Length => tracks.sort_by_key(|track| track.duration_ms),
            Sort::Added => tracks.sort_by_key(|track| date(track)),
        }
        let uris: Rc<[String]> = tracks.iter().map(|track| track.uri.clone()).collect();
        Listing {
            source: full.source.clone(),
            requested: uris.len(),
            loaded: uris.len(),
            uris,
            tracks,
            loading: Loading::Idle,
        }
    }

    /// What the list's heading says of its order and filter.
    pub(super) fn arrangement_summary(&self) -> String {
        let sort = self.sort.get().label();
        let filter = self.filter.with(|text| text.trim().to_owned());
        let total = self.full.borrow().as_ref().map(Listing::count);
        let mut parts = Vec::new();
        if let (false, Some(total)) = (filter.is_empty(), total) {
            let shown = self.listing.with(Listing::count);
            parts.push(format!("{shown} of {total} for “{filter}”"));
        }
        if !sort.is_empty() {
            parts.push(sort.to_owned());
        }
        if self.load_all.get() && total.is_none() && self.arranged() {
            parts.push("loading the whole list…".into());
        }
        parts.join(" · ")
    }
}

fn matches(track: &Track, filter: &str) -> bool {
    [&track.name, &track.artist_names(), &track.album.name]
        .iter()
        .any(|text| super::library_search::fold(text).contains(filter))
}
