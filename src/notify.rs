//! A desktop notification when a track starts, through the freedesktop
//! notification service on Linux; elsewhere, none.
use crate::spotify::{Error, Track};

/// Tells the desktop that `track` started, with its cover as the icon, if the
/// settings ask for it. The notification replaces the one of the track before.
pub(crate) async fn track_started(
    http: reqwest::Client,
    track: &Track,
    cover: Option<String>,
) -> Result<(), Error> {
    let settings = &crate::config::get().notifications;
    if !settings.enabled {
        return Ok(());
    }
    let summary = track.name.clone();
    let body = [track.artist_names(), track.album.name.clone()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let cover = cover.filter(|_| settings.covers);
    let timeout = i32::try_from(settings.timeout_ms).unwrap_or(i32::MAX);
    tokio::spawn(async move {
        let icon = match cover {
            Some(url) => icon(&http, &url).await.unwrap_or_default(),
            None => String::new(),
        };
        send(&summary, &body, &icon, timeout).await
    })
    .await?
}

/// The cover saved in `$XDG_CACHE_HOME/spt/covers`, which the notification daemon reads.
async fn icon(http: &reqwest::Client, url: &str) -> Option<String> {
    let name = url.rsplit('/').next().filter(|name| !name.is_empty())?;
    let dir = crate::config::xdg_dir("XDG_CACHE_HOME", ".cache")?.join("covers");
    let path = dir.join(format!("{name}.jpg"));
    if !path.exists() {
        let bytes = http.get(url).send().await.ok()?.bytes().await.ok()?;
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&path, bytes).ok()?;
    }
    Some(path.to_string_lossy().into_owned())
}

#[cfg(target_os = "linux")]
async fn send(summary: &str, body: &str, icon: &str, timeout: i32) -> Result<(), Error> {
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicU32, Ordering},
    };
    /// The last notification, which the next one replaces.
    static LAST: AtomicU32 = AtomicU32::new(0);
    let failed = |error: zbus::Error| Error::Notify(error.to_string());
    let connection = zbus::Connection::session().await.map_err(failed)?;
    let hints: HashMap<&str, zbus::zvariant::Value<'_>> = HashMap::new();
    let reply = connection
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                crate::COMMAND,
                LAST.load(Ordering::Relaxed),
                icon,
                summary,
                escape(body),
                Vec::<&str>::new(),
                hints,
                if timeout == 0 { -1 } else { timeout },
            ),
        )
        .await
        .map_err(failed)?;
    let id: u32 = reply.body().deserialize().map_err(failed)?;
    LAST.store(id, Ordering::Relaxed);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
async fn send(_: &str, _: &str, _: &str, _: i32) -> Result<(), Error> {
    Ok(())
}

/// The body may hold markup for some daemons: names keep their `&`, `<` and `>`.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_keep_their_signs() {
        assert_eq!(
            super::escape("Simon & <Garfunkel>"),
            "Simon &amp; &lt;Garfunkel&gt;"
        );
    }
}
