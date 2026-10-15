//! The view the player showed last and its selected row, which it opens again at
//! the next start. It lives in `$XDG_STATE_HOME/spt/view.json`: losing it only
//! means opening Liked Songs.
use super::listing::Source;
use crate::spotify::{Album, Artist, Playlist, Resume};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, io, path::Path, path::PathBuf, rc::Rc};

/// A view and the row selected in it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(super) struct Saved {
    #[serde(flatten)]
    view: View,
    #[serde(default)]
    pub(super) row: usize,
}

impl Saved {
    pub(super) fn new(source: &Source, row: usize) -> Self {
        Self {
            view: View::from(source),
            row,
        }
    }

    pub(super) fn source(&self) -> Source {
        Source::from(self.view.clone())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum View {
    Liked,
    Recent,
    Queue,
    Search { text: String },
    Library { text: String },
    Playlist { uri: String, name: String },
    Album { uri: String, name: String },
    Artist { uri: String, name: String },
    Show { uri: String, name: String },
}

impl From<&Source> for View {
    fn from(source: &Source) -> Self {
        match source {
            Source::Liked => Self::Liked,
            Source::Recent => Self::Recent,
            Source::Queue => Self::Queue,
            Source::Search(text) => Self::Search { text: text.clone() },
            Source::Library(text) => Self::Library { text: text.clone() },
            Source::Playlist(playlist) => Self::Playlist {
                uri: playlist.uri.clone(),
                name: playlist.name.clone(),
            },
            Source::Album(album) => Self::Album {
                uri: album.uri.clone(),
                name: album.name.clone(),
            },
            Source::Artist(artist) => Self::Artist {
                uri: artist.uri.clone(),
                name: artist.name.clone(),
            },
            Source::Show(show) => Self::Show {
                uri: show.uri.clone(),
                name: show.name.clone(),
            },
        }
    }
}

impl From<View> for Source {
    fn from(view: View) -> Self {
        match view {
            View::Liked => Self::Liked,
            View::Recent => Self::Recent,
            View::Queue => Self::Queue,
            View::Search { text } => Self::Search(text),
            View::Library { text } => Self::Library(text),
            View::Playlist { uri, name } => Self::Playlist(Rc::new(Playlist { uri, name })),
            View::Album { uri, name } => Self::Album(Rc::new(Album { uri, name })),
            View::Artist { uri, name } => Self::Artist(Rc::new(Artist { uri, name })),
            View::Show { uri, name } => Self::Show(Rc::new(Album { uri, name })),
        }
    }
}

fn path() -> Option<PathBuf> {
    state_file("view.json")
}

fn state_file(name: &str) -> Option<PathBuf> {
    crate::config::xdg_dir("XDG_STATE_HOME", ".local/state").map(|dir| dir.join(name))
}

/// Where an episode was left on this computer.
#[derive(Deserialize, Serialize)]
struct Position {
    position_ms: u64,
    #[serde(default)]
    played: bool,
}

/// Where the episodes were left on this computer, from `episodes.json`.
pub(super) fn recall_episodes() -> HashMap<String, Resume> {
    let Some(text) =
        state_file("episodes.json").and_then(|path| std::fs::read_to_string(path).ok())
    else {
        return HashMap::new();
    };
    let positions: HashMap<String, Position> = serde_json::from_str(&text).unwrap_or_default();
    positions
        .into_iter()
        .map(|(uri, position)| {
            let point = Resume {
                position_ms: position.position_ms,
                fully_played: position.played,
            };
            (uri, point)
        })
        .collect()
}

/// Saves where the episodes were left.
pub(super) fn remember_episodes(points: &HashMap<String, Resume>) -> io::Result<()> {
    let positions: HashMap<&String, Position> = points
        .iter()
        .map(|(uri, point)| {
            let position = Position {
                position_ms: point.position_ms,
                played: point.fully_played,
            };
            (uri, position)
        })
        .collect();
    let path = state_file("episodes.json")
        .ok_or_else(|| io::Error::other("no home folder to save the episodes in"))?;
    write_whole(&path, &serde_json::to_vec(&positions)?)
}

/// The dates of the likes taken back, from `unliked.json`, by URI.
pub(super) fn recall_unliked() -> HashMap<String, u32> {
    state_file("unliked.json")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Keeps the dates of the likes taken back, so that a like given again keeps its place.
pub(super) fn remember_unliked(dates: &HashMap<String, u32>) -> io::Result<()> {
    let path = state_file("unliked.json")
        .ok_or_else(|| io::Error::other("no home folder to keep the dates in"))?;
    write_whole(&path, &serde_json::to_vec(dates)?)
}

/// Writes a file whole or not at all: it goes to a file beside, then takes its name.
fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let partial = path.with_extension("partial");
    std::fs::write(&partial, bytes)?;
    std::fs::rename(partial, path)
}

/// The view saved last, if any could be read.
pub(super) fn recall() -> Option<Saved> {
    let text = std::fs::read_to_string(path()?).ok()?;
    serde_json::from_str(&text).ok()
}

/// Saves the view.
pub(super) fn remember(saved: &Saved) -> io::Result<()> {
    let path = path().ok_or_else(|| io::Error::other("no home folder to save the view in"))?;
    write_whole(&path, &serde_json::to_vec(saved)?)
}

#[cfg(test)]
mod tests {
    use super::{Saved, Source};
    use crate::spotify::Playlist;
    use std::rc::Rc;

    #[test]
    fn a_view_comes_back_as_it_left() {
        let sources = [
            Source::Liked,
            Source::Search("daft punk".into()),
            Source::Playlist(Rc::new(Playlist {
                uri: "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M".into(),
                name: "Today's Top Hits".into(),
            })),
        ];
        for source in sources {
            let text = serde_json::to_string(&Saved::new(&source, 12)).unwrap();
            let saved: Saved = serde_json::from_str(&text).unwrap();
            assert_eq!((saved.source(), saved.row), (source, 12));
        }
    }

    #[test]
    fn a_view_saved_without_its_row_still_reads() {
        let saved: Saved = serde_json::from_str(r#"{"kind":"liked"}"#).unwrap();
        assert_eq!((saved.source(), saved.row), (Source::Liked, 0));
    }
}
