use std::sync::Arc;

use druid::{
    widget::{CrossAxisAlignment, Flex, Label, LineBreaking, List},
    LensExt, LocalizedString, Menu, MenuItem, Selector, Size, Widget, WidgetExt,
};

use crate::{
    cmd,
    data::{
        Album, AlbumDetail, AlbumLink, AppState, ArtistLink, Cached, Ctx, Library, Nav, Playable,
        PlaybackOrigin, WithCtx,
    },
    ui::playable::PlayableIter,
    webapi::WebApi,
    widget::{Async, MyWidgetExt, RemoteImage},
};

use super::{artist, library, playable, theme, track, utils};

pub const LOAD_DETAIL: Selector<AlbumLink> = Selector::new("app.album.load-detail");

pub fn detail_widget() -> impl Widget<AppState> {
    Async::new(
        utils::spinner_widget,
        loaded_detail_widget,
        utils::error_widget,
    )
    .lens(
        Ctx::make(
            AppState::common_ctx,
            AppState::album_detail.then(AlbumDetail::album),
        )
        .then(Ctx::in_promise()),
    )
    .on_command_async(
        LOAD_DETAIL,
        |d| WebApi::global().get_album(&d.id),
        |_, data, d| data.album_detail.album.defer(d),
        |_, data, r| data.album_detail.album.update(r),
    )
}

fn loaded_detail_widget() -> impl Widget<WithCtx<Cached<Arc<Album>>>> {
    let album_cover = rounded_cover_widget(theme::grid(10.0))
        .lens(Ctx::data())
        .context_menu(album_ctx_menu);

    let album_artists = List::new(artist::link_widget).lens(Album::artists.in_arc());

    let album_date = Label::dynamic(|album: &Arc<Album>, _| album.release())
        .with_text_size(theme::TEXT_SIZE_SMALL);

    let album_label = Label::raw()
        .with_line_break_mode(LineBreaking::Clip)
        .with_text_size(theme::TEXT_SIZE_SMALL)
        .with_text_color(theme::PLACEHOLDER_COLOR)
        .lens(Album::label.in_arc());

    let album_info = Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_child(album_artists)
        .with_default_spacer()
        .with_child(album_date)
        .with_default_spacer()
        .with_child(album_label)
        .padding(theme::grid(1.0));

    let album_top = Flex::row()
        .with_spacer(theme::grid(4.2))
        .with_child(album_cover)
        .with_default_spacer()
        .with_flex_child(album_info.lens(Ctx::data()), 1.0);

    let album_tracks = playable::list_widget(playable::Display {
        track: track::Display {
            number: true,
            title: true,
            artist: true,
            ..track::Display::empty()
        },
    });

    Flex::column()
        .cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacer(theme::grid(1.0))
        .with_child(album_top)
        .with_spacer(theme::grid(1.0))
        .with_child(album_tracks)
        .lens(Ctx::map(Cached::data))
}

fn cover_widget(size: f64) -> impl Widget<Arc<Album>> {
    RemoteImage::new(utils::placeholder_widget(), move |album: &Arc<Album>, _| {
        album.image(size, size).map(|image| image.url.clone())
    })
    .fix_size(size, size)
}

fn rounded_cover_widget(size: f64) -> impl Widget<Arc<Album>> {
    cover_widget(size).clip(Size::new(size, size).to_rounded_rect(4.0))
}

pub fn album_widget(horizontal: bool) -> impl Widget<WithCtx<Arc<Album>>> {
    let album_cover_size = if horizontal { 16.0 } else { 6.0 };
    let album_cover = rounded_cover_widget(theme::grid(album_cover_size));

    // In grid mode use a dynamic label with ellipsis truncation so long names
    // never overflow the fixed-width card. In list mode use the raw lens-based
    // label so data updates are efficient.
    let album_name: Box<dyn Widget<Arc<Album>>> = if horizontal {
        Label::dynamic(|album: &Arc<Album>, _| truncate_ellipsis(&album.name, 22))
            .with_font(theme::UI_FONT_MEDIUM)
            .with_line_break_mode(LineBreaking::Clip)
            .boxed()
    } else {
        Label::raw()
            .with_font(theme::UI_FONT_MEDIUM)
            .with_line_break_mode(LineBreaking::Clip)
            .lens(Album::name.in_arc())
            .boxed()
    };

    // In grid mode render a single clipped/truncated line with all artists
    // joined by ", ". In list mode keep the original horizontal list widget.
    let album_artists: Box<dyn Widget<Arc<Album>>> = if horizontal {
        Label::dynamic(|album: &Arc<Album>, _| {
            let names: Vec<&str> = album.artists.iter().map(|a| a.name.as_ref()).collect();
            truncate_ellipsis(&names.join(", "), 26)
        })
        .with_text_size(theme::TEXT_SIZE_SMALL)
        .with_line_break_mode(LineBreaking::Clip)
        .boxed()
    } else {
        List::new(|| {
            Label::raw()
                .with_text_size(theme::TEXT_SIZE_SMALL)
                .with_line_break_mode(LineBreaking::WordWrap)
                .lens(ArtistLink::name)
        })
        .horizontal()
        .with_spacing(theme::grid(1.0))
        .lens(Album::artists.in_arc())
        .boxed()
    };

    let album_date = Label::<Arc<Album>>::dynamic(|album, _| album.release_year())
        .with_line_break_mode(LineBreaking::Clip)
        .with_text_size(theme::TEXT_SIZE_SMALL)
        .with_text_color(theme::PLACEHOLDER_COLOR);

    let album_layout = if horizontal {
        Flex::column()
            .with_child(album_cover)
            .with_default_spacer()
            .with_child(
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .with_child(album_name)
                    .with_spacer(1.0)
                    .with_child(album_artists)
                    .with_spacer(1.0)
                    .with_child(album_date)
                    // Fix sizing: keep a fixed height to prevent overflow but do
                    // NOT wrap in Align(CENTER), which was causing the centered
                    // appearance. cross_axis_alignment::Start on the column above
                    // already takes care of left-aligning the children.
                    .fix_size(theme::grid(16.0), theme::grid(8.0)),
            )
            .align_left()
    } else {
        Flex::row()
            .with_child(album_cover)
            .with_default_spacer()
            .with_flex_child(
                Flex::column()
                    .cross_axis_alignment(CrossAxisAlignment::Start)
                    .with_child(album_name)
                    .with_spacer(1.0)
                    .with_child(album_artists)
                    .with_spacer(1.0)
                    .with_child(album_date),
                1.0,
            )
            .align_left()
    };

    album_layout
        .padding(theme::grid(1.0))
        .lens(Ctx::data())
        .link()
        .rounded(theme::BUTTON_BORDER_RADIUS)
        .on_left_click(|ctx, _, album, _| {
            ctx.submit_command(cmd::NAVIGATE.with(Nav::AlbumDetail(album.data.link(), None)));
        })
        .context_menu(album_ctx_menu)
}

/// Truncates `s` to at most `max_chars` Unicode scalar values, appending the
/// ellipsis character `…` when the string is shortened. Used in fixed-width
/// grid cards where long names would otherwise overflow their container.
///
/// The `max_chars` limit is a character-count approximation; the exact number
/// of characters that fit depends on the font and glyph widths at runtime.
fn truncate_ellipsis(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        s.chars()
            .take(max_chars.saturating_sub(1))
            .collect::<String>()
            + "…"
    }
}

fn album_ctx_menu(album: &WithCtx<Arc<Album>>) -> Menu<AppState> {
    album_menu(&album.data, &album.ctx.library)
}

fn album_menu(album: &Arc<Album>, library: &Arc<Library>) -> Menu<AppState> {
    let mut menu = Menu::empty();

    for artist_link in &album.artists {
        let more_than_one_artist = album.artists.len() > 1;
        let title = if more_than_one_artist {
            LocalizedString::new("menu-item-show-artist-name")
                .with_placeholder(format!("Go to Artist \"{}\"", artist_link.name))
        } else {
            LocalizedString::new("menu-item-show-artist").with_placeholder("Go to Artist")
        };
        menu = menu.entry(
            MenuItem::new(title)
                .command(cmd::NAVIGATE.with(Nav::ArtistDetail(artist_link.to_owned()))),
        );
    }

    menu = menu.entry(
        MenuItem::new(
            LocalizedString::new("menu-item-copy-link").with_placeholder("Copy Link to Album"),
        )
        .command(cmd::COPY.with(album.url())),
    );

    menu = menu.separator();

    if library.contains_album(album) {
        menu = menu.entry(
            MenuItem::new(
                LocalizedString::new("menu-item-remove-from-library")
                    .with_placeholder("Remove Album from Library"),
            )
            .command(library::UNSAVE_ALBUM.with(album.link())),
        );
    } else {
        menu = menu.entry(
            MenuItem::new(
                LocalizedString::new("menu-item-save-to-library")
                    .with_placeholder("Save Album to Library"),
            )
            .command(library::SAVE_ALBUM.with(album.clone())),
        );
    }

    menu
}

impl PlayableIter for Arc<Album> {
    fn origin(&self) -> PlaybackOrigin {
        PlaybackOrigin::Album(self.link())
    }

    fn for_each(&self, mut cb: impl FnMut(Playable, usize)) {
        for (position, track) in self.clone().into_tracks_with_context().iter().enumerate() {
            cb(Playable::Track(track.clone()), position);
        }
    }

    fn count(&self) -> usize {
        self.tracks.len()
    }
}
