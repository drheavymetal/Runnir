//! One track, whoever is selling it.
//!
//! The player, the queue, the panel, MPRIS, the status bar and the share page all used
//! to be written in terms of `tidal::Track`, which was honest while TIDAL was the only
//! provider there was. It is not a small change to undo, but it is a shallow one: what
//! those six places actually want is a title, an artist, an album, a length and
//! something to play, and none of them care where it came from.
//!
//! The one place that does care is `play_one`, which has to pick a path — and that is
//! exactly the place where a `source` field reads as a decision rather than as a leak.

/// Where a track came from, which decides how it is fetched and what can be claimed
/// about it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Source {
    #[default]
    Tidal,
    Spotify,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Tidal => "TIDAL",
            Source::Spotify => "Spotify",
        }
    }

    /// Whether what this source serves has already lost information before it arrives.
    ///
    /// Spotify serves every Connect endpoint Ogg Vorbis 320, so no chain, however
    /// exclusive, can be bit-perfect — and the badge must not say it is.
    pub fn is_lossy(self) -> bool {
        matches!(self, Source::Spotify)
    }
}

/// A track in the queue.
///
/// `id` is a string rather than a number because a Spotify id is not one: it is
/// `spotify:track:4cOdK2wGLETKBW3PvgPWqT`. TIDAL's numeric ids go in as their decimal
/// spelling and come back out with `tidal_id()`, which is the only place that has to
/// know the difference.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Track {
    pub source: Source,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u32,
    /// The provider's own word for what it will serve: `HI_RES_LOSSLESS`, `LOSSLESS`,
    /// `HIGH`, `LOW` from TIDAL, `OGG 320` from Spotify. Never a promise — the badge
    /// reports what actually arrived.
    pub quality: String,
}

impl Track {
    /// The numeric id TIDAL's API wants. `None` for anything else, which is what stops a
    /// Spotify track being handed to a TIDAL endpoint as a zero.
    pub fn tidal_id(&self) -> Option<u64> {
        if self.source != Source::Tidal {
            return None;
        }
        self.id.parse().ok()
    }

    /// A stable identity for MPRIS and for "is this the same track". Provider-qualified,
    /// because a TIDAL id and a Spotify id can collide as strings and mean different
    /// songs.
    pub fn key(&self) -> String {
        match self.source {
            Source::Tidal => format!("tidal:{}", self.id),
            Source::Spotify => self.id.clone(),
        }
    }
}

impl From<crate::tidal::Track> for Track {
    fn from(t: crate::tidal::Track) -> Self {
        Track {
            source: Source::Tidal,
            id: t.id.to_string(),
            title: t.title,
            artist: t.artist,
            album: t.album,
            duration_secs: t.duration_secs,
            quality: t.quality,
        }
    }
}

impl From<crate::spotify::Track> for Track {
    fn from(t: crate::spotify::Track) -> Self {
        Track {
            source: Source::Spotify,
            id: t.uri,
            title: t.title,
            artist: t.artist,
            album: t.album,
            duration_secs: t.seconds,
            // Not asked for and not variable: this is what every Connect endpoint is
            // served, so writing it here rather than carrying an Option keeps the panel
            // from having to explain a blank.
            quality: "OGG 320".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tidal_track_keeps_its_number_and_a_spotify_one_never_gets_one() {
        let tidal: Track = crate::tidal::Track {
            id: 123456,
            title: "My Home Is In The Delta".into(),
            artist: "Muddy Waters".into(),
            ..Default::default()
        }
        .into();
        assert_eq!(tidal.tidal_id(), Some(123456));
        assert_eq!(tidal.source, Source::Tidal);

        let spotify: Track = crate::spotify::Track {
            uri: "spotify:track:4cOdK2wGLETKBW3PvgPWqT".into(),
            title: "My Home Is In The Delta".into(),
            ..Default::default()
        }
        .into();
        // The point of the None: handing this to a TIDAL endpoint as a 0 would ask for
        // somebody else's song rather than fail.
        assert_eq!(spotify.tidal_id(), None);
        assert_eq!(spotify.quality, "OGG 320");
    }

    /// Two providers can spell an id the same way and mean different songs, so identity
    /// has to carry the provider — MPRIS hands this out as a track id.
    #[test]
    fn identity_is_qualified_by_provider() {
        let a = Track { source: Source::Tidal, id: "42".into(), ..Default::default() };
        let b = Track { source: Source::Spotify, id: "42".into(), ..Default::default() };
        assert_ne!(a.key(), b.key());
    }

    #[test]
    fn only_spotify_is_lossy_and_the_badge_depends_on_it() {
        assert!(Source::Spotify.is_lossy());
        assert!(!Source::Tidal.is_lossy());
    }
}
