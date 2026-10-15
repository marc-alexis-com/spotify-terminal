# Changelog

## Unreleased

- **Likes.** A heart before every liked track, in the lists and the player;
  `H` likes the row or takes the like back, `*` does it for what plays. A like
  taken back keeps its date: liking the track again puts it back in its place.
- **Playlists.** `t` adds the row to a playlist, chosen in a picker that filters
  as you type; a new name creates the playlist.
- **Queue.** `u` and the sidebar's Up next show the queued tracks, then the rest
  of the list, as they will play.
- **Albums and podcasts** get a header: cover, makers, year, label, count.
- **Notifications** on Linux when a track starts, with its cover; configurable.
- **Podcasts.** Episodes start where this computer left them, and ✓ and ◐ mark
  the played and begun ones.
- **Fix.** A title too long for its column no longer runs into the next one.
- Changes to the library go through the session, like the desktop app: the
  public Web API answers this client with "too many requests".

## v0.2.0 — 2026-10-15

The first release as spotify-terminal, no longer a fork of fusor-rs/spotify-tui.

- **Settings.** `~/.config/spt/config.toml` sets the theme (nine colors), square
  or rounded corners, the player above or below, the sidebar and its width, the
  columns, the start view, covers, timings, every key, playback, the cache, the
  search and MPRIS. `spt check-config` checks it; `--config` reads another file.
- **Playback.** 320 kbit/s and normalised loudness by default; autoplay, gapless,
  initial volume and its steps, the fade around pause, the audio output
  (`spt devices` lists them), the device's name and type. Played tracks are
  cached, 1 GB by default.
- **Podcasts.** The arrows seek 30 seconds in an episode.
- **Memory.** The view and the row you left open again at the next start.
- **Artist pages**, library search, search as you type, grayed-out unplayable
  tracks, a fade around pause and resume, square covers.
- **Fixes.** A player still running 3 seconds after its terminal closed, Ctrl+C
  or `q` now stops; the device keeps one ID across starts.

## v0.1.1

The last version of fusor-rs/spotify-tui this project started from.
