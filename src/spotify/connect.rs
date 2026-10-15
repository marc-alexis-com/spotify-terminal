//! The account's playback as Spotify Connect sees it: what plays next, on which
//! devices; plus a track's lyrics and radio. All of it reads the session's own
//! services; only a transfer changes anything, and only where music plays.
use super::{Catalog, Error, catalog::run};
use http::Method;
use serde::Deserialize;
use std::collections::HashMap;

const TRACK_PREFIX: &str = "spotify:track:";

/// A device of the account that can play.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Device {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) active: bool,
    /// Whether it is this computer.
    pub(crate) here: bool,
}

/// One line of a track's lyrics and when it starts.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LyricLine {
    pub(crate) start_ms: u64,
    pub(crate) words: String,
}

#[derive(Deserialize)]
struct Cluster {
    active_device_id: Option<String>,
    #[serde(default)]
    devices: HashMap<String, ClusterDevice>,
    player_state: Option<PlayerState>,
}

#[derive(Deserialize)]
struct ClusterDevice {
    name: Option<String>,
    device_type: Option<String>,
}

#[derive(Deserialize)]
struct PlayerState {
    #[serde(default)]
    next_tracks: Vec<NextTrack>,
    context_uri: Option<String>,
    #[serde(default)]
    is_playing: bool,
    #[serde(default)]
    is_paused: bool,
}

#[derive(Deserialize)]
struct NextTrack {
    uri: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Lyrics {
    lyrics: LyricsBody,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LyricsBody {
    sync_type: Option<String>,
    lines: Vec<Line>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Line {
    start_time_ms: String,
    words: String,
}

impl Catalog {
    async fn cluster(&self) -> Result<Cluster, Error> {
        let session = self.session.clone();
        run(async move {
            let body = session
                .spclient()
                .request_as_json(&Method::GET, "/connect-state/v1/cluster", None, None)
                .await?;
            Ok(serde_json::from_slice(&body)?)
        })
        .await
    }

    /// What plays next on the account, wherever it plays: the queue first.
    pub(crate) async fn up_next(&self) -> Result<Vec<String>, Error> {
        let next = self
            .cluster()
            .await?
            .player_state
            .map(|state| state.next_tracks)
            .unwrap_or_default();
        let playable = |track: &NextTrack| {
            track.uri.as_deref().is_some_and(|uri| {
                uri.starts_with(TRACK_PREFIX) || uri.starts_with("spotify:episode:")
            })
        };
        Ok(next
            .into_iter()
            .filter(playable)
            .filter_map(|track| track.uri)
            .collect())
    }

    /// The account's devices, this computer among them.
    pub(crate) async fn devices(&self) -> Result<Vec<Device>, Error> {
        let cluster = self.cluster().await?;
        let here = self.session.device_id().to_owned();
        let active = cluster.active_device_id.unwrap_or_default();
        let mut devices: Vec<Device> = cluster
            .devices
            .into_iter()
            .map(|(id, device)| Device {
                active: id == active,
                here: id == here,
                name: device.name.unwrap_or_else(|| id.clone()),
                kind: device.device_type.unwrap_or_default().to_lowercase(),
                id,
            })
            .collect();
        devices.sort_by_key(|device| (!device.here, device.name.clone()));
        Ok(devices)
    }

    /// The context the account plays from, such as a playlist or Liked Songs.
    pub(crate) async fn playing_context(&self) -> Result<Option<String>, Error> {
        Ok(self
            .cluster()
            .await?
            .player_state
            .and_then(|state| state.context_uri)
            .filter(|uri| !uri.is_empty()))
    }

    /// Moves the playback from the device in use to `to`, playing there if it
    /// played here. Spotify is told so rather than left to read the device it
    /// leaves: a librespot device it takes the music from has paused by then,
    /// and the music arrived paused.
    pub(crate) async fn transfer(&self, to: String) -> Result<(), Error> {
        let cluster = self.cluster().await?;
        let from = cluster
            .active_device_id
            .ok_or_else(|| Error::Missing("device in use".into()))?;
        let playing = cluster
            .player_state
            .is_some_and(|state| state.is_playing && !state.is_paused);
        let session = self.session.clone();
        run(async move {
            let paused = if playing { "resume" } else { "pause" };
            let body = serde_json::json!({ "transfer_options": { "restore_paused": paused } });
            session
                .spclient()
                .request_as_json(
                    &Method::POST,
                    &format!("/connect-state/v1/connect/transfer/from/{from}/to/{to}"),
                    None,
                    Some(&body.to_string()),
                )
                .await?;
            Ok(())
        })
        .await
    }

    /// The lyrics of a track, line by line; none when Spotify has none, and
    /// all at zero when they are not timed.
    pub(crate) async fn lyrics(&self, uri: String) -> Result<Vec<LyricLine>, Error> {
        let Some(id) = uri.strip_prefix(TRACK_PREFIX).map(str::to_owned) else {
            return Ok(Vec::new());
        };
        let session = self.session.clone();
        run(async move {
            let path = format!("/color-lyrics/v2/track/{id}?format=json&market=from_token");
            let body = match session
                .spclient()
                .request_as_json(&Method::GET, &path, None, None)
                .await
            {
                Ok(body) => body,
                // Spotify answers "not found" for a track without lyrics.
                Err(error) if error.kind == librespot_core::error::ErrorKind::NotFound => {
                    return Ok(Vec::new());
                }
                Err(error) => return Err(error.into()),
            };
            let lyrics: Lyrics = serde_json::from_slice(&body)?;
            let timed = lyrics.lyrics.sync_type.as_deref() == Some("LINE_SYNCED");
            Ok(lyrics
                .lyrics
                .lines
                .into_iter()
                .map(|line| LyricLine {
                    start_ms: if timed {
                        line.start_time_ms.parse().unwrap_or(0)
                    } else {
                        0
                    },
                    words: line.words,
                })
                .collect())
        })
        .await
    }

    /// Tracks like `track`, as Spotify's radio of it plays them.
    pub(crate) async fn radio(&self, track: &str) -> Result<Vec<String>, Error> {
        let id = track.strip_prefix(TRACK_PREFIX).unwrap_or(track);
        self.context_tracks(format!("spotify:station:track:{id}"))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::{Cluster, Lyrics};

    #[test]
    fn reads_the_cluster_and_lyrics() {
        let cluster: Cluster = serde_json::from_str(
            r#"{"active_device_id":"a","devices":{"a":{"name":"Phone","device_type":"SMARTPHONE"}},
               "player_state":{"next_tracks":[{"uri":"spotify:track:x","provider":"queue"},
               {"uri":"spotify:delimiter","provider":"context"}]}}"#,
        )
        .unwrap();
        assert_eq!(cluster.devices["a"].name.as_deref(), Some("Phone"));
        assert_eq!(cluster.player_state.unwrap().next_tracks.len(), 2);
        let lyrics: Lyrics = serde_json::from_str(
            r#"{"lyrics":{"syncType":"LINE_SYNCED","lines":[{"startTimeMs":"64070","words":"Around the world"}]}}"#,
        )
        .unwrap();
        assert_eq!(lyrics.lyrics.lines[0].words, "Around the world");
    }
}
