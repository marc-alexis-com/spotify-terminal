# Changelog

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
