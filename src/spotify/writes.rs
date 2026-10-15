//! What changes the account: likes and playlists. It goes through the session's
//! own services, as Spotify's desktop app does; the public Web API turns this
//! client away with "too many requests" from the first call.
use super::{Catalog, Error, catalog::run};
use http::{HeaderMap, HeaderValue, Method, header::CONTENT_TYPE};
use librespot_core::SpotifyId;
use librespot_protocol::playlist4_external::{
    Add, CreateListReply, Delta, Item, ListAttributes, ListChanges, ListUpdateRequest, Op,
    SelectedListContent, op,
};
use protobuf::{CodedOutputStream, EnumOrUnknown, Message, MessageField};

const PLAYLIST_PREFIX: &str = "spotify:playlist:";
const EPISODE_PREFIX: &str = "spotify:episode:";
/// The collection of Liked Songs, and the one of saved episodes.
const LIKED_SET: &str = "collection";
const EPISODES_SET: &str = "listenlater";
const COLLECTION_TYPE: &str = "application/vnd.collection-v2.spotify.proto";

/// Where a podcast episode was left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Resume {
    pub(crate) position_ms: u64,
    pub(crate) fully_played: bool,
}

impl Catalog {
    /// Adds tracks to Liked Songs, or episodes to Your Episodes; or takes them out.
    pub(crate) async fn set_saved(&self, uris: Vec<String>, saved: bool) -> Result<(), Error> {
        let session = self.session.clone();
        run(async move {
            let username = session.username();
            let (episodes, tracks): (Vec<_>, Vec<_>) = uris
                .into_iter()
                .partition(|uri| uri.starts_with(EPISODE_PREFIX));
            let sets = [(LIKED_SET, tracks), (EPISODES_SET, episodes)];
            for (set, uris) in sets.into_iter().filter(|(_, uris)| !uris.is_empty()) {
                let body = collection_write(&username, set, &uris, !saved)?;
                let mut headers = HeaderMap::new();
                headers.insert(CONTENT_TYPE, HeaderValue::from_static(COLLECTION_TYPE));
                session
                    .spclient()
                    .request(
                        &Method::POST,
                        "/collection/v2/write",
                        Some(headers),
                        Some(&body),
                    )
                    .await?;
            }
            Ok(())
        })
        .await
    }

    /// Adds tracks or episodes at the end of a playlist.
    pub(crate) async fn add_to_playlist(
        &self,
        playlist: &str,
        uris: Vec<String>,
    ) -> Result<(), Error> {
        let (session, playlist) = (self.session.clone(), playlist.to_owned());
        run(async move {
            let id = playlist.strip_prefix(PLAYLIST_PREFIX).unwrap_or(&playlist);
            let current = session
                .spclient()
                .get_playlist(&SpotifyId::from_base62(id)?)
                .await?;
            let revision = SelectedListContent::parse_from_bytes(&current)?.revision;
            let changes = changes(revision, add(uris, false));
            session
                .spclient()
                .request_with_protobuf(
                    &Method::POST,
                    &format!("/playlist/v2/playlist/{id}/changes"),
                    None,
                    &changes,
                )
                .await?;
            Ok(())
        })
        .await
    }

    /// Creates a playlist, puts it first in the library and returns its URI.
    pub(crate) async fn create_playlist(&self, name: String) -> Result<String, Error> {
        let session = self.session.clone();
        run(async move {
            let request = ListUpdateRequest {
                attributes: MessageField::some(ListAttributes {
                    name: Some(name),
                    ..ListAttributes::default()
                }),
                ..ListUpdateRequest::default()
            };
            let reply = session
                .spclient()
                .request_with_protobuf(&Method::POST, "/playlist/v2/playlist", None, &request)
                .await?;
            let uri = CreateListReply::parse_from_bytes(&reply)?.uri().to_owned();
            let username = session.username();
            let rootlist = session.spclient().get_rootlist(0, Some(1)).await?;
            let revision = SelectedListContent::parse_from_bytes(&rootlist)?.revision;
            session
                .spclient()
                .request_with_protobuf(
                    &Method::POST,
                    &format!("/playlist/v2/user/{username}/rootlist/changes"),
                    None,
                    &changes(revision, add(vec![uri.clone()], true)),
                )
                .await?;
            Ok(uri)
        })
        .await
    }
}

/// An addition of `uris` at the end of a list, or at its start.
fn add(uris: Vec<String>, first: bool) -> Op {
    Op {
        kind: Some(EnumOrUnknown::new(op::Kind::ADD)),
        add: MessageField::some(Add {
            items: uris
                .into_iter()
                .map(|uri| Item {
                    uri: Some(uri),
                    ..Item::default()
                })
                .collect(),
            add_last: Some(!first),
            add_first: Some(first),
            ..Add::default()
        }),
        ..Op::default()
    }
}

fn changes(revision: Option<Vec<u8>>, op: Op) -> ListChanges {
    ListChanges {
        base_revision: revision,
        deltas: vec![Delta {
            ops: vec![op],
            ..Delta::default()
        }],
        want_resulting_revisions: Some(false),
        ..ListChanges::default()
    }
}

/// A `WriteRequest` of collection2v2.proto, which librespot does not compile:
/// the user, the set, then each item with its URI, the time and whether it goes.
fn collection_write(
    username: &str,
    set: &str,
    uris: &[String],
    removed: bool,
) -> Result<Vec<u8>, Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            i32::try_from(since.as_secs()).unwrap_or(i32::MAX)
        });
    let mut body = Vec::new();
    let mut out = CodedOutputStream::vec(&mut body);
    out.write_string(1, username)?;
    out.write_string(2, set)?;
    for uri in uris {
        let mut item = Vec::new();
        let mut fields = CodedOutputStream::vec(&mut item);
        fields.write_string(1, uri)?;
        fields.write_int32(2, now)?;
        fields.write_bool(3, removed)?;
        fields.flush()?;
        drop(fields);
        out.write_bytes(3, &item)?;
    }
    out.flush()?;
    drop(out);
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::collection_write;

    #[test]
    fn a_write_request_names_the_user_the_set_and_the_items() {
        let body =
            collection_write("me", "collection", &["spotify:track:a".into()], false).unwrap();
        // Field 1 (user) "me", field 2 (set) "collection", then one item.
        assert_eq!(&body[..4], &[0x0A, 2, b'm', b'e']);
        assert_eq!(&body[4..6], &[0x12, 10]);
        assert_eq!(body[16], 0x1A);
        let item = &body[18..];
        assert_eq!(&item[..2], &[0x0A, 15]);
        assert!(item.ends_with(&[0x18, 0]));
    }
}
