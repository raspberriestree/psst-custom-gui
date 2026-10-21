mod album;
mod artist;
pub mod config;
mod ctx;
mod find;
mod id;
mod nav;
mod playback;
mod playlist;
mod promise;
mod recommend;
mod search;
mod show;
mod slider_scroll_scale;
mod track;
mod user;
pub mod utils;

use std::{
    fmt::Display,
    mem,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use druid::{
    im::{HashSet, Vector},
    Data, Lens,
};
use psst_core::{item_id::ItemId, session::SessionService};

pub use crate::data::{
    album::{Album, AlbumDetail, AlbumLink, AlbumType, DatePrecision},
    artist::{
        Artist, ArtistAlbums, ArtistDetail, ArtistInfo, ArtistLink, ArtistOverview, ArtistStats,
    },
    config::{
        AlbumSortCriteria, AlbumViewMode, AudioQuality, Authentication, Config, Preferences,
        PreferencesTab, Theme,
    },
    ctx::Ctx,
    find::{FindQuery, Finder, MatchFindQuery},
    nav::{Nav, Route, SpotifyUrl},
    playback::{
        NowPlaying, Playable, PlayableMatcher, Playback, PlaybackOrigin, PlaybackPayload,
        PlaybackState, QueueBehavior, QueueEntry,
    },
    playlist::{
        Playlist, PlaylistAddTrack, PlaylistDetail, PlaylistLink, PlaylistRemoveTrack,
        PlaylistTracks,
    },
    promise::{Promise, PromiseState},
    recommend::{
        Range, Recommend, Recommendations, RecommendationsKnobs, RecommendationsParams,
        RecommendationsRequest, Toggled,
    },
    search::{Search, SearchResults, SearchTopic},
    show::{Episode, EpisodeId, EpisodeLink, Show, ShowDetail, ShowEpisodes, ShowLink},
    slider_scroll_scale::SliderScrollScale,
    track::{AudioAnalysis, Track, TrackId, TrackLines},
    user::{PublicUser, UserProfile},
    utils::{Cached, Float64, Image, Page},
};
use crate::ui::credits::TrackCredits;

pub const ALERT_DURATION: Duration = Duration::from_secs(5);

#[derive(Clone, Data, Lens)]
pub struct AppState {
    #[data(ignore)]
    pub session: SessionService,
    pub nav: Nav,
    pub history: Vector<Nav>,
    pub config: Config,
    pub preferences: Preferences,
    pub playback: Playback,
    pub search: Search,
    pub recommend: Recommend,
    pub album_detail: AlbumDetail,
    pub artist_detail: ArtistDetail,
    pub playlist_detail: PlaylistDetail,
    pub show_detail: ShowDetail,
    pub library: Arc<Library>,
    pub common_ctx: Arc<CommonCtx>,
    pub home_detail: HomeDetail,
    pub alerts: Vector<Alert>,
    pub finder: Finder,
    pub added_queue: Vector<QueueEntry>,
    pub lyrics: Promise<Vector<TrackLines>>,
    pub credits: Option<TrackCredits>,
}

impl AppState {
    pub fn default_with_config(config: Config) -> Self {
        let library = Arc::new(Library {
            user_profile: Promise::Empty,
            saved_albums: Promise::Empty,
            saved_tracks: Promise::Empty,
            saved_shows: Promise::Empty,
            playlists: Promise::Empty,
        });
        let common_ctx = Arc::new(CommonCtx {
            now_playing: None,
            library: Arc::clone(&library),
            show_track_cover: config.show_track_cover,
            nav: Nav::Home,
        });
        let playback = Playback {
            state: PlaybackState::Stopped,
            now_playing: None,
            queue_behavior: config.queue_behavior,
            queue: Vector::new(),
            volume: config.volume,
        };
        Self {
            session: SessionService::empty(),
            nav: Nav::Home,
            history: Vector::new(),
            config,
            preferences: Preferences {
                active: PreferencesTab::General,
                cache: None,
                cache_size: Promise::Empty,
                auth: Authentication::new(),
                lastfm_auth_result: None,
            },
            playback,
            added_queue: Vector::new(),
            search: Search {
                input: "".into(),
                topic: None,
                results: Promise::Empty,
            },
            recommend: Recommend {
                knobs: Default::default(),
                results: Promise::Empty,
            },
            home_detail: HomeDetail {
                made_for_you: Promise::Empty,
                user_top_mixes: Promise::Empty,
                best_of_artists: Promise::Empty,
                recommended_stations: Promise::Empty,
                your_shows: Promise::Empty,
                shows_that_you_might_like: Promise::Empty,
                uniquely_yours: Promise::Empty,
                jump_back_in: Promise::Empty,
                user_top_tracks: Promise::Empty,
                user_top_artists: Promise::Empty,
            },
            album_detail: AlbumDetail {
                album: Promise::Empty,
            },
            artist_detail: ArtistDetail {
                artist: Promise::Empty,
                albums: Promise::Empty,
                overview: Promise::Empty,
            },
            playlist_detail: PlaylistDetail {
                playlist: Promise::Empty,
                tracks: Promise::Empty,
            },
            show_detail: ShowDetail {
                show: Promise::Empty,
                episodes: Promise::Empty,
            },
            library,
            common_ctx,
            alerts: Vector::new(),
            finder: Finder::new(),
            lyrics: Promise::Empty,
            credits: None,
        }
    }
}

impl AppState {
    pub fn navigate(&mut self, nav: &Nav) {
        if &self.nav != nav {
            let previous = mem::replace(&mut self.nav, nav.to_owned());
            self.history.push_back(previous);
            self.config.last_route.replace(nav.to_owned());
            Arc::make_mut(&mut self.common_ctx).nav = nav.to_owned();
        }
    }

    pub fn navigate_back(&mut self) {
        if let Some(mut nav) = self.history.pop_back() {
            if let Nav::SearchResults(query) = &nav {
                if SpotifyUrl::parse(query).is_some() {
                    nav = self.history.pop_back().unwrap_or(Nav::Home);
                }
            }

            if let Nav::AlbumDetail(album, _) = nav {
                nav = Nav::AlbumDetail(album, None);
            }

            self.nav = nav;
            self.config.last_route.replace(self.nav.to_owned());
            Arc::make_mut(&mut self.common_ctx).nav = self.nav.clone();
        }
    }

    pub fn refresh_all(&mut self) {
        self.album_detail.album = Promise::Empty;
        self.artist_detail.overview = Promise::Empty;
        self.artist_detail.albums = Promise::Empty;
        self.artist_detail.artist = Promise::Empty;
        self.playlist_detail.playlist = Promise::Empty;
        self.playlist_detail.tracks = Promise::Empty;
        self.show_detail.episodes = Promise::Empty;
        self.show_detail.show = Promise::Empty;
    }

    pub fn refresh_playlist(&mut self) {
        self.playlist_detail.tracks = Promise::Empty;
        self.playlist_detail.playlist = Promise::Empty;
    }
}

impl AppState {
    pub fn queued_entry(&self, item_id: ItemId) -> Option<QueueEntry> {
        if let Some(queued) = self
            .playback
            .queue
            .iter()
            .find(|queued| queued.item.id() == item_id)
            .cloned()
        {
            Some(queued)
        } else {
            self.added_queue
                .iter()
                .find(|queued| queued.item.id() == item_id)
                .cloned()
        }
    }

    pub fn add_queued_entry(&mut self, queue_entry: QueueEntry) {
        self.added_queue.push_back(queue_entry);
    }

    pub fn loading_playback(&mut self, item: Playable, origin: PlaybackOrigin) {
        self.common_ctx_mut().now_playing.take();
        self.playback.state = PlaybackState::Loading;
        self.playback.now_playing.replace(NowPlaying {
            item,
            origin,
            progress: Duration::default(),
            library: Arc::clone(&self.library),
        });
    }

    pub fn start_playback(&mut self, item: Playable, origin: PlaybackOrigin, progress: Duration) {
        self.common_ctx_mut().now_playing.replace(item.clone());
        self.playback.state = PlaybackState::Playing;
        self.playback.now_playing.replace(NowPlaying {
            item,
            origin,
            progress,
            library: Arc::clone(&self.library),
        });
    }

    pub fn progress_playback(&mut self, progress: Duration) {
        if let Some(now_playing) = &mut self.playback.now_playing {
            now_playing.progress = progress;
        }
    }

    pub fn pause_playback(&mut self) {
        self.playback.state = PlaybackState::Paused;
    }

    pub fn resume_playback(&mut self) {
        self.playback.state = PlaybackState::Playing;
    }

    pub fn block_playback(&mut self) {
        // TODO: Figure out how to signal blocked playback properly.
    }

    pub fn stop_playback(&mut self) {
        self.playback.state = PlaybackState::Stopped;
        self.playback.now_playing.take();
        self.common_ctx_mut().now_playing.take();
    }

    pub fn set_queue_behavior(&mut self, queue_behavior: QueueBehavior) {
        self.playback.queue_behavior = queue_behavior;
        self.config.queue_behavior = queue_behavior;
        self.config.save();
    }
}

impl AppState {
    pub fn common_ctx_mut(&mut self) -> &mut CommonCtx {
        Arc::make_mut(&mut self.common_ctx)
    }

    pub fn with_library_mut(&mut self, func: impl FnOnce(&mut Library)) {
        func(Arc::make_mut(&mut self.library));
        self.library_updated();
    }

    fn library_updated(&mut self) {
        if let Some(now_playing) = &mut self.playback.now_playing {
            now_playing.library = Arc::clone(&self.library);
        }
        self.common_ctx_mut().library = Arc::clone(&self.library);
    }
}

impl AppState {
    pub fn add_alert(&mut self, message: impl Display, style: AlertStyle) {
        let alert = Alert {
            message: message.to_string().into(),
            style,
            id: Alert::fresh_id(),
            created_at: Instant::now(),
        };
        self.alerts.push_back(alert);
    }

    pub fn info_alert(&mut self, message: impl Display) {
        self.add_alert(message, AlertStyle::Info);
    }

    pub fn error_alert(&mut self, message: impl Display) {
        self.add_alert(message, AlertStyle::Error);
    }

    pub fn dismiss_alert(&mut self, id: usize) {
        self.alerts.retain(|a| a.id != id);
    }

    pub fn cleanup_alerts(&mut self) {
        let now = Instant::now();
        self.alerts
            .retain(|alert| now.duration_since(alert.created_at) < ALERT_DURATION);
    }
}

#[derive(Clone, Data, Lens)]
pub struct Library {
    pub user_profile: Promise<UserProfile>,
    pub playlists: Promise<Vector<Playlist>>,
    pub saved_albums: Promise<SavedAlbums>,
    pub saved_tracks: Promise<SavedTracks>,
    pub saved_shows: Promise<Shows>,
}

impl Library {
    pub fn add_track(&mut self, track: Arc<Track>) {
        if let Some(saved) = self.saved_tracks.resolved_mut() {
            saved.set.insert(track.id);
            saved.tracks.push_front(track);
        }
    }

    pub fn remove_track(&mut self, track_id: &TrackId) {
        if let Some(saved) = self.saved_tracks.resolved_mut() {
            saved.set.remove(track_id);
            saved.tracks.retain(|t| &t.id != track_id);
        }
    }

    pub fn contains_track(&self, track: &Track) -> bool {
        if let Some(saved) = self.saved_tracks.resolved() {
            saved.set.contains(&track.id)
        } else {
            false
        }
    }

    pub fn add_album(&mut self, album: Arc<Album>) {
        if let Some(saved) = self.saved_albums.resolved_mut() {
            saved.add(album);
        }
    }

    pub fn remove_album(&mut self, album_id: &str) {
        if let Some(saved) = self.saved_albums.resolved_mut() {
            saved.remove(album_id);
        }
    }

    pub fn contains_album(&self, album: &Album) -> bool {
        if let Some(saved) = self.saved_albums.resolved() {
            saved.set.contains(&album.id)
        } else {
            false
        }
    }

    pub fn add_show(&mut self, show: Arc<Show>) {
        if let Some(saved) = self.saved_shows.resolved_mut() {
            saved.set.insert(show.id.clone());
            saved.shows.push_front(show);
        }
    }

    pub fn remove_show(&mut self, show_id: &str) {
        if let Some(saved) = self.saved_shows.resolved_mut() {
            saved.set.remove(show_id);
            saved.shows.retain(|a| a.id.as_ref() != show_id);
        }
    }

    pub fn contains_show(&self, show: &Show) -> bool {
        if let Some(saved) = self.saved_shows.resolved() {
            saved.set.contains(&show.id)
        } else {
            false
        }
    }

    pub fn writable_playlists(&self) -> Vec<&Playlist> {
        if let Some(saved) = self.playlists.resolved() {
            saved
                .iter()
                .filter(|playlist| {
                    self.user_profile
                        .resolved()
                        .map(|user| playlist.owner.id == user.id)
                        .unwrap_or(false)
                        || playlist.collaborative
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn add_playlist(&mut self, playlist: Playlist) {
        if let Some(playlists) = self.playlists.resolved_mut() {
            playlists.push_back(playlist);
        }
    }

    pub fn remove_from_playlist(&mut self, id: &str) {
        if let Some(playlists) = self.playlists.resolved_mut() {
            playlists.retain(|p| p.id.as_ref() != id);
        }
    }

    pub fn rename_playlist(&mut self, link: PlaylistLink) {
        if let Some(saved) = self.playlists.resolved_mut() {
            for playlist in saved.iter_mut() {
                if playlist.id == link.id {
                    playlist.name = link.name;
                    break;
                }
            }
        }
    }

    pub fn is_created_by_user(&self, playlist: &Playlist) -> bool {
        if let Some(profile) = self.user_profile.resolved() {
            profile.id == playlist.owner.id
        } else {
            false
        }
    }

    pub fn contains_playlist(&self, playlist: &Playlist) -> bool {
        if let Some(playlists) = self.playlists.resolved() {
            playlists.iter().any(|p| p.id == playlist.id)
        } else {
            false
        }
    }

    pub fn increment_playlist_track_count(&mut self, link: &PlaylistLink) {
        if let Some(saved) = self.playlists.resolved_mut() {
            if let Some(playlist) = saved.iter_mut().find(|p| p.id == link.id) {
                playlist.track_count = playlist.track_count.map(|count| count + 1);
            }
        }
    }

    pub fn decrement_playlist_track_count(&mut self, link: &PlaylistLink) {
        if let Some(saved) = self.playlists.resolved_mut() {
            if let Some(playlist) = saved.iter_mut().find(|p| p.id == link.id) {
                playlist.track_count = playlist.track_count.map(|count| count.saturating_sub(1));
            }
        }
    }
}

impl Default for Library {
    fn default() -> Self {
        Library {
            user_profile: Promise::Empty,
            playlists: Promise::Empty,
            saved_albums: Promise::Empty,
            saved_tracks: Promise::Empty,
            saved_shows: Promise::Empty,
        }
    }
}

#[derive(Clone, Default, Data, Lens)]
pub struct SavedTracks {
    pub tracks: Vector<Arc<Track>>,
    pub set: HashSet<TrackId>,
}

impl SavedTracks {
    pub fn new(tracks: Vector<Arc<Track>>) -> Self {
        let set = tracks.iter().map(|t| t.id).collect();
        Self { tracks, set }
    }
}

#[derive(Clone, Default, Data, Lens)]
pub struct SavedAlbums {
    /// Original order returned by the Spotify API (most recently saved first).
    /// Kept so we can restore the DateAdded ordering after the user switches
    /// back from a name/artist sort.
    #[data(ignore)]
    original_order: Vector<Arc<Album>>,
    /// The currently displayed (possibly sorted) album list.
    pub albums: Vector<Arc<Album>>,
    pub set: HashSet<Arc<str>>,
}

impl SavedAlbums {
    pub fn new(albums: Vector<Arc<Album>>) -> Self {
        let set = albums.iter().map(|a| a.id.clone()).collect();
        Self {
            original_order: albums.clone(),
            albums,
            set,
        }
    }

    pub fn add(&mut self, album: Arc<Album>) {
        self.set.insert(album.id.clone());
        self.original_order.push_front(album.clone());
        self.albums.push_front(album);
    }

    pub fn remove(&mut self, album_id: &str) {
        self.set.remove(album_id);
        self.original_order.retain(|a| a.id.as_ref() != album_id);
        self.albums.retain(|a| a.id.as_ref() != album_id);
    }

    /// Re-order `albums` according to `criteria` and `order`.
    ///
    /// `original_order` is always the reference; calling this method multiple
    /// times with the same arguments is idempotent.
    pub fn sort(
        &mut self,
        criteria: config::AlbumSortCriteria,
        order: config::SortOrder,
    ) {
        use config::{AlbumSortCriteria, SortOrder};

        if criteria == AlbumSortCriteria::DateAdded {
            // Restore the API order (newest saved = index 0).
            self.albums = self.original_order.clone();
            if order == SortOrder::Ascending {
                // Ascending date = oldest first → reverse the API list.
                let mut v: Vec<Arc<Album>> = self.albums.iter().cloned().collect();
                v.reverse();
                self.albums = v.into_iter().collect();
            }
        } else {
            let mut v: Vec<Arc<Album>> = self.original_order.iter().cloned().collect();
            v.sort_by(|a, b| {
                let cmp = match criteria {
                    AlbumSortCriteria::AlbumName => a
                        .name
                        .to_lowercase()
                        .cmp(&b.name.to_lowercase()),
                    AlbumSortCriteria::Artist => {
                        let a_name = a
                            .artists
                            .front()
                            .map(|ar| ar.name.to_lowercase())
                            .unwrap_or_default();
                        let b_name = b
                            .artists
                            .front()
                            .map(|ar| ar.name.to_lowercase())
                            .unwrap_or_default();
                        a_name.cmp(&b_name)
                    }
                    AlbumSortCriteria::DateAdded => unreachable!(),
                };
                if order == SortOrder::Descending {
                    cmp.reverse()
                } else {
                    cmp
                }
            });
            self.albums = v.into_iter().collect();
        }
    }
}

#[derive(Clone, Default, Data, Lens)]
pub struct Shows {
    pub shows: Vector<Arc<Show>>,
    pub set: HashSet<Arc<str>>,
}

impl Shows {
    pub fn new(shows: Vector<Arc<Show>>) -> Self {
        let set = shows.iter().map(|a| a.id.clone()).collect();
        Self { shows, set }
    }
}

#[derive(Clone, Data)]
pub struct CommonCtx {
    pub now_playing: Option<Playable>,
    pub library: Arc<Library>,
    pub show_track_cover: bool,
    pub nav: Nav,
}

impl CommonCtx {
    pub fn is_playing(&self, item: &Playable) -> bool {
        matches!(&self.now_playing, Some(i) if i.same(item))
    }
}

pub type WithCtx<T> = Ctx<Arc<CommonCtx>, T>;

#[derive(Clone, Data, Lens)]
pub struct HomeDetail {
    pub made_for_you: Promise<MixedView>,
    pub user_top_mixes: Promise<MixedView>,
    pub best_of_artists: Promise<MixedView>,
    pub recommended_stations: Promise<MixedView>,
    pub uniquely_yours: Promise<MixedView>,
    pub your_shows: Promise<MixedView>,
    pub shows_that_you_might_like: Promise<MixedView>,
    pub jump_back_in: Promise<MixedView>,
    pub user_top_tracks: Promise<Vector<Arc<Track>>>,
    pub user_top_artists: Promise<Vector<Artist>>,
}

#[derive(Clone, Data, Lens)]
pub struct MixedView {
    pub title: Arc<str>,
    pub playlists: Vector<Playlist>,
    pub artists: Vector<Artist>,
    pub albums: Vector<Arc<Album>>,
    pub shows: Vector<Arc<Show>>,
}

static ALERT_ID: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Data, Lens)]
pub struct Alert {
    pub id: usize,
    pub message: Arc<str>,
    pub style: AlertStyle,
    pub created_at: Instant,
}

impl Alert {
    fn fresh_id() -> usize {
        ALERT_ID.fetch_add(1, Ordering::SeqCst)
    }
}

#[derive(Clone, Data, Eq, PartialEq)]
pub enum AlertStyle {
    Error,
    Info,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_album(id: &str, name: &str, artist: &str) -> Arc<Album> {
        Arc::new(Album {
            id: Arc::from(id),
            name: Arc::from(name),
            album_type: crate::data::AlbumType::Album,
            images: Vector::new(),
            artists: Vector::from(vec![ArtistLink {
                id: Arc::from(artist),
                name: Arc::from(artist),
            }]),
            copyrights: Vector::new(),
            label: Arc::from(""),
            tracks: Vector::new(),
            release_date: None,
            release_date_precision: None,
        })
    }

    #[test]
    fn test_saved_albums_sort() {
        let a1 = dummy_album("1", "Zebra", "Bob");
        let a2 = dummy_album("2", "Apple", "Charlie");
        let a3 = dummy_album("3", "Banana", "Alice");

        let mut saved = SavedAlbums::new(Vector::from(vec![a1.clone(), a2.clone(), a3.clone()]));

        // DateAdded Descending (default): should match original order [a1, a2, a3]
        saved.sort(config::AlbumSortCriteria::DateAdded, config::SortOrder::Descending);
        assert_eq!(saved.albums[0].name.as_ref(), "Zebra");
        assert_eq!(saved.albums[1].name.as_ref(), "Apple");
        assert_eq!(saved.albums[2].name.as_ref(), "Banana");

        // DateAdded Ascending: reversed original order [a3, a2, a1]
        saved.sort(config::AlbumSortCriteria::DateAdded, config::SortOrder::Ascending);
        assert_eq!(saved.albums[0].name.as_ref(), "Banana");
        assert_eq!(saved.albums[1].name.as_ref(), "Apple");
        assert_eq!(saved.albums[2].name.as_ref(), "Zebra");

        // AlbumName Ascending: Apple, Banana, Zebra
        saved.sort(config::AlbumSortCriteria::AlbumName, config::SortOrder::Ascending);
        assert_eq!(saved.albums[0].name.as_ref(), "Apple");
        assert_eq!(saved.albums[1].name.as_ref(), "Banana");
        assert_eq!(saved.albums[2].name.as_ref(), "Zebra");

        // AlbumName Descending: Zebra, Banana, Apple
        saved.sort(config::AlbumSortCriteria::AlbumName, config::SortOrder::Descending);
        assert_eq!(saved.albums[0].name.as_ref(), "Zebra");
        assert_eq!(saved.albums[1].name.as_ref(), "Banana");
        assert_eq!(saved.albums[2].name.as_ref(), "Apple");

        // Artist Ascending: Alice (Banana), Bob (Zebra), Charlie (Apple)
        saved.sort(config::AlbumSortCriteria::Artist, config::SortOrder::Ascending);
        assert_eq!(saved.albums[0].name.as_ref(), "Banana");
        assert_eq!(saved.albums[1].name.as_ref(), "Zebra");
        assert_eq!(saved.albums[2].name.as_ref(), "Apple");

        // Artist Descending: Charlie (Apple), Bob (Zebra), Alice (Banana)
        saved.sort(config::AlbumSortCriteria::Artist, config::SortOrder::Descending);
        assert_eq!(saved.albums[0].name.as_ref(), "Apple");
        assert_eq!(saved.albums[1].name.as_ref(), "Zebra");
        assert_eq!(saved.albums[2].name.as_ref(), "Banana");
    }

    #[test]
    fn test_saved_albums_add_remove() {
        let a1 = dummy_album("1", "Zebra", "Bob");
        let a2 = dummy_album("2", "Apple", "Charlie");
        let mut saved = SavedAlbums::new(Vector::from(vec![a1.clone()]));

        // Add an album
        saved.add(a2.clone());
        assert_eq!(saved.albums.len(), 2);
        assert!(saved.set.contains("2"));

        // Sort by AlbumName Ascending
        saved.sort(config::AlbumSortCriteria::AlbumName, config::SortOrder::Ascending);
        assert_eq!(saved.albums[0].name.as_ref(), "Apple");
        assert_eq!(saved.albums[1].name.as_ref(), "Zebra");

        // Remove album
        saved.remove("2");
        assert_eq!(saved.albums.len(), 1);
        assert!(!saved.set.contains("2"));

        // Sort again by DateAdded
        saved.sort(config::AlbumSortCriteria::DateAdded, config::SortOrder::Descending);
        assert_eq!(saved.albums.len(), 1);
        assert_eq!(saved.albums[0].name.as_ref(), "Zebra");
    }
}
