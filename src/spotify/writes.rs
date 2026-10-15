//! What changes the account: likes and playlists. It goes through the session's
//! own services, as Spotify's desktop app does; the public Web API turns this
//! client away with "too many requests" from the first call.
use super::{Catalog, Error, catalog::run};
use http::{
    HeaderMap, HeaderValue, Method,
    header::{ACCEPT, CONTENT_TYPE},
};
use librespot_core::SpotifyId;
use librespot_protocol::playlist4_external::{
    Add, CreateListReply, Delta, Item, ListAttributes, ListAttributesPartialState, ListChanges,
    ListUpdateRequest, Mov, Op, Rem, SelectedListContent, UpdateListAttributes, op,
};
use protobuf::{CodedOutputStream, EnumOrUnknown, Message, MessageField};
use std::collections::HashMap;

const PLAYLIST_PREFIX: &str = "spotify:playlist:";
const EPISODE_PREFIX: &str = "spotify:episode:";
/// The collection of Liked Songs, and the one of saved episodes.
const LIKED_SET: &str = "collection";
const EPISODES_SET: &str = "listenlater";
const COLLECTION_TYPE: &str = "application/vnd.collection-v2.spotify.proto";
/// Items in one page of the collection.
const PAGE_SIZE: i32 = 500;

/// Which way a track moves in a playlist.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Direction {
    Up,
    Down,
}

/// Where a podcast episode was left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Resume {
    pub(crate) position_ms: u64,
    pub(crate) fully_played: bool,
}

impl Catalog {
    /// Likes a track, or saves an episode, dated `added_at` (seconds since 1970):
    /// the date sets its place in Liked Songs, so a like taken back and given
    /// again keeps its place when its first date comes back. `None` dates it now.
    pub(crate) async fn save(&self, uri: String, added_at: Option<u32>) -> Result<(), Error> {
        self.collection_item(uri, added_at.unwrap_or_else(now), false)
            .await
    }

    /// Takes a like back, or an episode out of Your Episodes.
    pub(crate) async fn unsave(&self, uri: String) -> Result<(), Error> {
        self.collection_item(uri, now(), true).await
    }

    async fn collection_item(
        &self,
        uri: String,
        added_at: u32,
        removed: bool,
    ) -> Result<(), Error> {
        let session = self.session.clone();
        run(async move {
            let set = if uri.starts_with(EPISODE_PREFIX) {
                EPISODES_SET
            } else {
                LIKED_SET
            };
            let body = collection_write(&session.username(), set, &uri, added_at, removed)?;
            session
                .spclient()
                .request(
                    &Method::POST,
                    "/collection/v2/write",
                    Some(collection_headers()),
                    Some(&body),
                )
                .await?;
            Ok(())
        })
        .await
    }

    /// Liked Songs with the date each track was liked, by URI.
    pub(crate) async fn liked_dates(&self) -> Result<HashMap<String, u32>, Error> {
        run(collection(self.session.clone(), LIKED_SET)).await
    }

    /// The episodes saved to Your Episodes, last saved first.
    pub(crate) async fn saved_episodes(&self) -> Result<Vec<String>, Error> {
        let saved = run(collection(self.session.clone(), EPISODES_SET)).await?;
        let mut saved: Vec<_> = saved.into_iter().collect();
        saved.sort_by_key(|(_, added_at)| std::cmp::Reverse(*added_at));
        Ok(saved.into_iter().map(|(uri, _)| uri).collect())
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

    /// Takes the first `uri` out of a playlist.
    pub(crate) async fn remove_from_playlist(
        &self,
        playlist: &str,
        uri: String,
    ) -> Result<(), Error> {
        self.change_playlist(playlist, move |items| {
            let index = items.iter().position(|item| *item == uri)?;
            Some(Op {
                kind: Some(EnumOrUnknown::new(op::Kind::REM)),
                rem: MessageField::some(Rem {
                    from_index: Some(i32::try_from(index).ok()?),
                    length: Some(1),
                    items: vec![Item {
                        uri: Some(uri),
                        ..Item::default()
                    }],
                    ..Rem::default()
                }),
                ..Op::default()
            })
        })
        .await
    }

    /// Moves the first `uri` of a playlist one place up, or down.
    pub(crate) async fn move_in_playlist(
        &self,
        playlist: &str,
        uri: String,
        direction: Direction,
    ) -> Result<(), Error> {
        self.change_playlist(playlist, move |items| {
            let index = items.iter().position(|item| *item == uri)?;
            // The target is where the item goes before, counted before it moves.
            let to = match direction {
                Direction::Up => index.checked_sub(1)?,
                Direction::Down => (index + 2 <= items.len()).then_some(index + 2)?,
            };
            Some(Op {
                kind: Some(EnumOrUnknown::new(op::Kind::MOV)),
                mov: MessageField::some(Mov {
                    from_index: Some(i32::try_from(index).ok()?),
                    length: Some(1),
                    to_index: Some(i32::try_from(to).ok()?),
                    ..Mov::default()
                }),
                ..Op::default()
            })
        })
        .await
    }

    /// Gives a playlist a new name.
    pub(crate) async fn rename_playlist(&self, playlist: &str, name: String) -> Result<(), Error> {
        self.change_playlist(playlist, move |_| {
            Some(Op {
                kind: Some(EnumOrUnknown::new(op::Kind::UPDATE_LIST_ATTRIBUTES)),
                update_list_attributes: MessageField::some(UpdateListAttributes {
                    new_attributes: MessageField::some(ListAttributesPartialState {
                        values: MessageField::some(ListAttributes {
                            name: Some(name),
                            ..ListAttributes::default()
                        }),
                        ..ListAttributesPartialState::default()
                    }),
                    ..UpdateListAttributes::default()
                }),
                ..Op::default()
            })
        })
        .await
    }

    /// Reads a playlist, then sends the change `op` makes of its items, on its
    /// current revision; `op` returns nothing when the change cannot be made.
    async fn change_playlist(
        &self,
        playlist: &str,
        op: impl FnOnce(&[String]) -> Option<Op> + Send + 'static,
    ) -> Result<(), Error> {
        let (session, playlist) = (self.session.clone(), playlist.to_owned());
        run(async move {
            let id = playlist.strip_prefix(PLAYLIST_PREFIX).unwrap_or(&playlist);
            let current = session
                .spclient()
                .get_playlist(&SpotifyId::from_base62(id)?)
                .await?;
            let current = SelectedListContent::parse_from_bytes(&current)?;
            let items: Vec<String> = current
                .contents
                .items
                .iter()
                .map(|item| item.uri().to_owned())
                .collect();
            let op = op(&items).ok_or_else(|| Error::Missing("place for this change".into()))?;
            session
                .spclient()
                .request_with_protobuf(
                    &Method::POST,
                    &format!("/playlist/v2/playlist/{id}/changes"),
                    None,
                    &changes(current.revision, op),
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

fn now() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| {
            u32::try_from(since.as_secs()).unwrap_or(u32::MAX)
        })
}

fn collection_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(COLLECTION_TYPE));
    headers.insert(ACCEPT, HeaderValue::from_static(COLLECTION_TYPE));
    headers
}

/// A `WriteRequest` of collection2v2.proto, which librespot does not compile:
/// the user, the set, then one item with its URI, its date and whether it goes.
fn collection_write(
    username: &str,
    set: &str,
    uri: &str,
    added_at: u32,
    removed: bool,
) -> Result<Vec<u8>, Error> {
    let mut item = Vec::new();
    let mut fields = CodedOutputStream::vec(&mut item);
    fields.write_string(1, uri)?;
    fields.write_int32(2, i32::try_from(added_at).unwrap_or(i32::MAX))?;
    fields.write_bool(3, removed)?;
    fields.flush()?;
    drop(fields);
    let mut body = Vec::new();
    let mut out = CodedOutputStream::vec(&mut body);
    out.write_string(1, username)?;
    out.write_string(2, set)?;
    out.write_bytes(3, &item)?;
    out.flush()?;
    drop(out);
    Ok(body)
}

/// A collection set with the date of each item, by URI.
async fn collection(
    session: librespot_core::Session,
    set: &'static str,
) -> Result<HashMap<String, u32>, Error> {
    let mut liked = HashMap::new();
    let mut page = String::new();
    loop {
        let body = page_request(&session.username(), set, &page)?;
        let reply = session
            .spclient()
            .request(
                &Method::POST,
                "/collection/v2/paging",
                Some(collection_headers()),
                Some(&body),
            )
            .await?;
        page = read_page(&reply, &mut liked);
        if page.is_empty() {
            return Ok(liked);
        }
    }
}

/// A `PageRequest`: the user, the set, where the last page ended, and how many.
fn page_request(username: &str, set: &str, page: &str) -> Result<Vec<u8>, Error> {
    let mut body = Vec::new();
    let mut out = CodedOutputStream::vec(&mut body);
    out.write_string(1, username)?;
    out.write_string(2, set)?;
    out.write_string(3, page)?;
    out.write_int32(4, PAGE_SIZE)?;
    out.flush()?;
    drop(out);
    Ok(body)
}

/// Adds the items of a `PageResponse` that are still there, and returns where
/// the next page starts, or nothing after the last.
fn read_page(reply: &[u8], liked: &mut HashMap<String, u32>) -> String {
    let mut next = String::new();
    for (field, value) in fields(reply) {
        match (field, value) {
            (1, Field::Bytes(item)) => liked.extend(read_item(&item)),
            (2, Field::Bytes(token)) => next = String::from_utf8_lossy(&token).into(),
            _ => {}
        }
    }
    next
}

/// A `CollectionItem` still in the collection: its URI and date.
fn read_item(item: &[u8]) -> Option<(String, u32)> {
    let (mut uri, mut added_at, mut removed) = (String::new(), 0, false);
    for (field, value) in fields(item) {
        match (field, value) {
            (1, Field::Bytes(text)) => uri = String::from_utf8_lossy(&text).into(),
            (2, Field::Number(number)) => added_at = u32::try_from(number).unwrap_or(0),
            (3, Field::Number(number)) => removed = number != 0,
            _ => {}
        }
    }
    (!removed).then_some((uri, added_at))
}

enum Field {
    Number(u64),
    Bytes(Vec<u8>),
}

/// The fields of a protobuf message, as numbers and bytes; it stops at a kind
/// of field the collection never sends.
fn fields(buf: &[u8]) -> Vec<(u64, Field)> {
    let mut stream = protobuf::CodedInputStream::from_bytes(buf);
    let mut found = Vec::new();
    while let Ok(Some(tag)) = stream.read_raw_tag_or_eof() {
        let value = match tag & 7 {
            0 => stream.read_raw_varint64().map(Field::Number),
            2 => stream.read_bytes().map(Field::Bytes),
            _ => break,
        };
        match value {
            Ok(value) => found.push((u64::from(tag >> 3), value)),
            Err(_) => break,
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::{collection_write, page_request, read_page};
    use std::collections::HashMap;

    #[test]
    fn a_write_request_names_the_user_the_set_and_the_item() {
        let body =
            collection_write("me", "collection", "spotify:track:a", 1_654_795_474, false).unwrap();
        // Field 1 (user) "me", field 2 (set) "collection", then the item.
        assert_eq!(&body[..4], &[0x0A, 2, b'm', b'e']);
        assert_eq!(&body[4..6], &[0x12, 10]);
        assert_eq!(body[16], 0x1A);
        let item = &body[18..];
        assert_eq!(&item[..2], &[0x0A, 15]);
        assert!(item.ends_with(&[0x18, 0]));
    }

    #[test]
    fn a_page_keeps_what_is_still_liked_with_its_date() {
        // Two items, one removed, then the token of the next page.
        let item = |uri: &str, at: u32, removed: bool| {
            let mut bytes = vec![0x0A, u8::try_from(uri.len()).unwrap()];
            bytes.extend(uri.bytes());
            bytes.push(0x10);
            let mut at = at;
            while at >= 0x80 {
                bytes.push(u8::try_from(at & 0x7F).unwrap() | 0x80);
                at >>= 7;
            }
            bytes.push(u8::try_from(at).unwrap());
            bytes.extend([0x18, u8::from(removed)]);
            bytes
        };
        let mut reply = Vec::new();
        for entry in [
            item("spotify:track:a", 1_654_795_474, false),
            item("spotify:track:b", 5, true),
        ] {
            reply.push(0x0A);
            reply.push(u8::try_from(entry.len()).unwrap());
            reply.extend(entry);
        }
        reply.extend([0x12, 3, b'n', b'x', b't']);
        let mut liked = HashMap::new();
        assert_eq!(read_page(&reply, &mut liked), "nxt");
        assert_eq!(liked.len(), 1);
        assert_eq!(liked["spotify:track:a"], 1_654_795_474);
        assert!(!page_request("me", "collection", "").unwrap().is_empty());
    }
}
