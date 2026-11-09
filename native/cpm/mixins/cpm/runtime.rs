// CPM for Horizon: draws the local player with a Customizable Player Models model
// (`<game dir>/player_models/*.cpmmodel`). This file is both the mod's Cargo library (its tests)
// and the module the bundle adds to the game as `game::cpm_mod`, so it names no game type: the
// helpers (frame.rs, listened.rs, player_submit.rs, pose_probe.rs) are the edges to the game.
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cpm_core::glam::{Mat4, Vec3};
use cpm_core::{ArmorSlot, BoxTexture, Equipment, ItemAnchor, ItemSlot, LoadedModel, Posed, PoseInput, PosedBox, Sheet, ShoulderAnchor};

/// The mod's id: its data folder and its row in Options > Mods.
pub const ID: &str = "cpm";
/// Shown in Options > Mods.
pub const VERSION: &str = "0.1.0";
pub const DESCRIPTION: &str =
    "Draws you with a Customizable Player Models model (.cpmmodel) from the player_models folder; a key opens its gesture menu.";
/// The sprite the texture is uploaded into (`Game::native_mod_image`), at its top left.
pub const SPRITE: &str = "horizon/modimage";
/// That sprite's side, in pixels.
pub const SPRITE_PX: u32 = 1024;
/// The opaque white patch to the right of the texture, for untextured (colour) boxes.
const WHITE_PX: u32 = 4;
/// Another mod may draw into the same sprite: upload again this often while a model is shown.
pub const REUPLOAD: Duration = Duration::from_secs(5);
/// Packed light for a glowing box: full sky and block light (`mobs::render::FULL_BRIGHT`).
pub const FULL_BRIGHT: u32 = 240 | 240 << 8;

#[derive(Default)]
struct State {
    /// The model file the player chose.
    selected: Option<PathBuf>,
    /// The file `model` (or `error`) is for; `None` when nothing was tried yet.
    loaded: Option<PathBuf>,
    model: Option<LoadedModel>,
    error: Option<String>,
    /// When the texture was last handed to the game; `None`: upload at the next frame.
    uploaded: Option<Instant>,
    /// The first `clock_ms` call: animation time counts from there.
    start: Option<Instant>,
    /// `restore_once` ran.
    started: bool,
    /// The last conversion's outcome, for the settings page ([`note_convert_result`]).
    last_convert: Option<ConvertResult>,
    /// How many conversions had ended ([`Job::finished`]) when [`poll_convert`] last looked.
    convert_seen: u64,
    /// A converted model too big for a gist link's inline form: (model, size) for the popup.
    gist_popup: Option<(PathBuf, usize)>,
    /// The gist popup on screen (the mod screen is open for it).
    gist: Option<GistPopup>,
    /// What the popup's input hooks (which cannot change the game) left for the frame hook.
    gist_requests: Vec<GistRequest>,
    /// The mod's data folder ([`set_data_dir`]), which holds the gist cache.
    data_dir: Option<PathBuf>,
    /// The selected link model waiting for one of its links' downloads.
    waiting: Option<Waiting>,
    /// A model just loaded and its saved parameters were not restored yet ([`restore_params`]).
    restore_pending: bool,
    /// The parameters last restored or saved, with the model file they belong to
    /// ([`take_params_to_save`] saves only a change from this).
    params_saved: Option<(String, Vec<u8>)>,
    /// The gesture menu was asked for ([`request_menu`]).
    menu_requested: bool,
    /// The gesture menu on screen (the mod screen is open for it).
    menu: Option<Menu>,
    /// The selected file's description, read before it is decoded (for [`accept`]).
    desc: Option<String>,
    /// The loaded model was converted by an older converter ([`old_conversion`]).
    old_conversion: bool,
    /// The local player's identities ([`set_own_identities`]), which player heads are matched to.
    own: Vec<Identity>,
    /// The input the world draw last posed the model with ([`record_input`]).
    last_input: Option<PoseInput>,
    /// The loaded model's preview fit: the first frame its preview drew ([`preview_fit`]), kept for the load.
    preview_fit: Option<PreviewFit>,
    /// The Models picker, on the gesture menu's screen ([`open_picker`]).
    picker: Option<Picker>,
    /// The image last made for [`SPRITE`] ([`texture_upload`]); `None`: make it again.
    composed: Option<Composed>,
    /// The image changed: send it at the next frame.
    upload_due: bool,
    /// When the image was last sent.
    last_upload: Option<Instant>,
    /// The local player's downloaded skin and the model stored in it ([`set_skin`]).
    skin: Option<SkinState>,
    /// The skin `model` (or `error`) is for, when it is the skin's model ([`is_skin`]).
    loaded_skin: Option<String>,
}

/// The `model` setting's value for the model stored in the player's skin ([`skin_selection`]).
/// Not a `.cpmmodel` name, so never a file of the folder.
pub const SKIN_SETTING: &str = "<skin>";
/// The skin's model on the Model button and its picker tile.
pub const SKIN_MODEL: &str = "Your skin's model";
/// The settings page's line while the skin's model is the one drawn (chosen, or nothing chosen).
pub const SKIN_IN_USE: &str = "Using the model stored in your skin";
/// The skin's model in error lines (`cpm: skin model: <error>`).
const SKIN_NAME: &str = "skin model";

/// The local player's skin, from the game's skin cache (skin_probe.rs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinInput {
    /// Its texture id (`skins/<digest>`): a new one is a new skin.
    pub location: String,
    /// `PlayerSkin.model` is slim: the slim channel is read first.
    pub slim: bool,
    /// The raw downloaded PNG (the data lives in it; the game's processed copy loses it).
    pub png: Vec<u8>,
    /// The skin as the game draws it (size, RGBA8): the texture of a skin model without its own.
    pub texture: Option<([u32; 2], Vec<u8>)>,
}

struct SkinState {
    location: String,
    slim: bool,
    texture: Option<([u32; 2], Vec<u8>)>,
    /// The model in the skin as a `.cpmmodel`, or why its data does not read; `None`: no model.
    model: Option<Result<Vec<u8>, String>>,
}

/// The selection that stands for the skin's model ([`SKIN_SETTING`]).
pub fn skin_selection() -> PathBuf {
    PathBuf::from(SKIN_SETTING)
}

/// Whether `path` is [`skin_selection`].
pub fn is_skin(path: &Path) -> bool {
    path == Path::new(SKIN_SETTING)
}

/// The local player's skin changed (`None`: no downloaded skin, as a default or offline one):
/// reads the model stored in it (`ModelDefinitionLoader.loadModel(Image, Player)`, the slim or
/// wide channel per `slim`). A PNG that does not decode holds no model (logged).
pub fn set_skin(skin: Option<SkinInput>) {
    let state = skin.map(|k| {
        let ty = if k.slim { cpm_core::format::SkinType::Slim } else { cpm_core::format::SkinType::Default };
        let model = match cpm_core::skin::Image::decode(&k.png) {
            Ok(img) => match cpm_core::skin::read_skin_data(&img, ty) {
                Ok(Some(data)) => Some(Ok(cpm_core::skin::container(&data))),
                Ok(None) => None,
                Err(e) => Some(Err(e.to_string())),
            },
            Err(e) => {
                // (An empty one: the file did not read, as the probe logged.)
                if !k.png.is_empty() {
                    log::warn!("cpm: cannot read your skin {}: {e}", k.location);
                }
                None
            }
        };
        SkinState { location: k.location, slim: k.slim, texture: k.texture, model }
    });
    // A picker open on the old skin keeps its tiles until it opens again.
    STATE.with_borrow_mut(|s| s.skin = state);
}

/// Whether `location` (slim or not) is the skin [`set_skin`] last read: the probe reads the file
/// again only for a new one.
pub fn skin_known(location: &str, slim: bool) -> bool {
    STATE.with_borrow(|s| s.skin.as_ref().is_some_and(|k| k.location == location && k.slim == slim))
}

/// Whether a skin was given ([`set_skin`]); the probe clears it only then.
pub fn skin_set() -> bool {
    STATE.with_borrow(|s| s.skin.is_some())
}

fn skin_has_model(s: &State) -> bool {
    s.skin.as_ref().is_some_and(|k| k.model.is_some())
}

/// What draws: the chosen file; the skin's model when it is chosen or nothing is (CPM's "skin
/// model" when no model is set), while the skin holds one; else nothing (the vanilla player).
fn effective(s: &State) -> Option<PathBuf> {
    match &s.selected {
        Some(p) if !is_skin(p) => Some(p.clone()),
        _ => skin_has_model(s).then(skin_selection),
    }
}

/// The selection that draws ([`effective`]): what the Model button shows and cycles from.
pub fn active_selection() -> Option<PathBuf> {
    STATE.with_borrow(effective)
}

/// Whether the skin's model is the one drawn (chosen, or nothing chosen). Only the tests ask (the
/// settings page reads it from [`active_selection`]).
#[allow(dead_code)]
pub fn skin_model_in_use() -> bool {
    STATE.with_borrow(|s| effective(s).is_some_and(|p| is_skin(&p)))
}

/// The Model button's cycle: the skin's model first while the skin holds one, then `models`.
pub fn model_cycle(models: Vec<PathBuf>) -> Vec<PathBuf> {
    let skin = STATE.with_borrow(skin_has_model);
    skin.then(skin_selection).into_iter().chain(models).collect()
}

/// A selection's name in messages: the file's name, [`SKIN_NAME`] for the skin's model.
fn shown_name(path: Option<&Path>) -> String {
    match path {
        Some(p) if is_skin(p) => SKIN_NAME.to_owned(),
        _ => file_name(path),
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Chooses the model file to draw (`None`: the vanilla player).
pub fn select(path: Option<PathBuf>) {
    STATE.with_borrow_mut(|s| s.selected = path);
}

/// The model file chosen. Only the tests ask (the game shows and cycles [`active_selection`]).
#[allow(dead_code)]
pub fn selected() -> Option<PathBuf> {
    STATE.with_borrow(|s| s.selected.clone())
}

/// Loads the selected model when the selection changed. A failure is logged once
/// (`cpm: <file>: <error>`) and kept for [`last_error`]; no model is drawn then.
///
/// A model with links (a `.link.cpmmodel`, or link parts of any CPM loader: gist, GitHub file,
/// paste, Pastebin, models CDN) loads from their cached texts ([`cache_path`]); a link without
/// one asks for a download ([`start_fetch`]) and the vanilla player is drawn until the download
/// ends ([`finish_fetch`]): a later call then resolves the next link, or caches the texts and loads
/// the model. A failed download is `cpm: <file>: gist <link>: <reason>` (`link <link>` for the
/// other loaders), tried again when the model is selected again.
pub fn ensure_loaded() {
    STATE.with_borrow_mut(|s| {
        let want = effective(s);
        // The skin's model loads again for a new skin.
        let want_skin = want.as_deref().filter(|p| is_skin(p)).and_then(|_| s.skin.as_ref()).map(|k| k.location.clone());
        if want == s.loaded && want_skin == s.loaded_skin {
            poll_link(s);
            return;
        }
        s.loaded = want.clone();
        s.loaded_skin = want_skin;
        s.model = None;
        s.error = None;
        s.uploaded = None;
        // The image is the old model's: made again for the new one (never patched).
        s.composed = None;
        s.waiting = None;
        s.restore_pending = false;
        s.params_saved = None;
        s.desc = None;
        s.old_conversion = false;
        s.preview_fit = None;
        let Some(path) = want else { return };
        let name = shown_name(Some(&path));
        let bytes = if is_skin(&path) {
            // The skin's model, as a `.cpmmodel` (links resolve as any model's).
            match s.skin.as_ref().and_then(|k| k.model.clone()) {
                Some(Ok(bytes)) => bytes,
                Some(Err(e)) => return fail(s, format!("cpm: {name}: {e}")),
                None => return,
            }
        } else {
            match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(e) => return fail(s, format!("cpm: {name}: {e}")),
            }
        };
        s.desc = cpm_core::format::read_cpmmodel(&bytes).ok().map(|f| f.desc);
        match cpm_core::load(&bytes) {
            Ok(model) => accept(s, &name, model),
            Err(cpm_core::CpmError::NeedsLink(link)) => load_link(s, &name, bytes, link),
            Err(e) => fail(s, format!("cpm: {name}: {e}")),
        }
    });
}

/// Keeps `model` when it has a texture ([`check_texture`]).
fn accept(s: &mut State, name: &str, mut model: LoadedModel) {
    // The skin's model without a texture of its own draws with the skin (`ModelDefinition`).
    if s.loaded.as_deref().is_some_and(is_skin)
        && let Some((size, rgba)) = s.skin.as_ref().and_then(|k| k.texture.clone())
    {
        cpm_core::skin::use_skin_texture(&mut model, size, rgba);
    }
    match check_texture(name, &model) {
        Ok(()) => {
            log::info!("cpm: loaded {name} ({})", model.name);
            s.model = Some(model);
            s.preview_fit = None;
            s.restore_pending = true;
            if s.desc.as_deref().and_then(converted_version).is_some_and(|v| v < CONVERTER_VERSION) {
                log::warn!("cpm: {name}: converted by an older version of the converter — convert it again");
                s.old_conversion = true;
            }
        }
        Err(e) => fail(s, e),
    }
}

/// A link model waiting for a download: the link being downloaded, the model file's bytes, and
/// the texts of its links downloaded so far (cached once the model loads).
struct Waiting {
    link: String,
    bytes: Vec<u8>,
    got: Vec<(String, Vec<u8>)>,
}

/// A link model's error line: `gist <link>` for a gist, `link <link>` for the other loaders.
fn gist_error(name: &str, link: &str, reason: impl std::fmt::Display) -> String {
    let kind = if link.starts_with("git:") { "gist" } else { "link" };
    format!("cpm: {name}: {kind} {link}: {reason}")
}

/// The model `bytes` (file `name`) has a link part `link` it cannot resolve itself: resolve its
/// links from the cache, downloading each one missing in turn.
fn load_link(s: &mut State, name: &str, bytes: Vec<u8>, link: String) {
    if let Err(e) = cpm_core::link::link_urls(&link) {
        return fail(s, gist_error(name, &link, e));
    }
    resolve(s, name, bytes, Vec::new());
}

/// Loads `bytes` with every link resolved from `got` (downloaded this time) or the cache. The
/// first link found in neither is downloaded and the model waits for it; once all resolve, the
/// downloaded texts are cached. A failure names the last link asked for and drops the cached
/// copies read (selecting the model again downloads them afresh).
fn resolve(s: &mut State, name: &str, bytes: Vec<u8>, got: Vec<(String, Vec<u8>)>) {
    let data_dir = s.data_dir.clone();
    let read_caches = RefCell::new(Vec::<PathBuf>::new());
    let last = RefCell::new(String::new());
    let missing = RefCell::new(None::<String>);
    let text = |link: &str| -> cpm_core::Result<Vec<u8>> {
        *last.borrow_mut() = link.to_owned();
        if let Some((_, t)) = got.iter().find(|(l, _)| l == link) {
            return Ok(t.clone());
        }
        cpm_core::link::link_urls(link)?;
        // No data folder known (not from the game): nowhere to cache, but it still downloads.
        let cache = data_dir.as_deref().and_then(|d| cache_path(d, link)).filter(|c| c.is_file());
        if let Some(cache) = cache {
            let t = std::fs::read(&cache)
                .map_err(|e| cpm_core::CpmError::Invalid(format!("cannot read the cache: {e}")));
            read_caches.borrow_mut().push(cache);
            return t;
        }
        *missing.borrow_mut() = Some(link.to_owned());
        Err(cpm_core::CpmError::NeedsLink(link.to_owned()))
    };
    let result = cpm_core::link::load_with_links(&bytes, &text);
    let (last, missing) = (last.into_inner(), missing.into_inner());
    match result {
        Ok(model) => {
            for (link, t) in &got {
                if let Some(cache) = data_dir.as_deref().and_then(|d| cache_path(d, link))
                    && let Err(e) = write_cache(&cache, t)
                {
                    log::warn!("{}", gist_error(name, link, e));
                }
            }
            accept(s, name, model);
        }
        Err(cpm_core::CpmError::NeedsLink(link)) if missing.as_deref() == Some(link.as_str()) => {
            start_fetch(&link);
            s.waiting = Some(Waiting { link, bytes, got });
            poll_link(s);
        }
        Err(e) => {
            for cache in read_caches.into_inner() {
                let _ = std::fs::remove_file(cache);
            }
            fail(s, gist_error(name, &last, e));
        }
    }
}

/// The waiting link model's download ended: resolve the model again with it, or fail.
fn poll_link(s: &mut State) {
    let Some(result) = s.waiting.as_ref().and_then(|w| take_fetch_result(&w.link)) else { return };
    let Some(Waiting { link, bytes, mut got }) = s.waiting.take() else { return };
    let name = shown_name(s.loaded.as_deref());
    match result {
        Ok(text) => {
            got.push((link, text));
            resolve(s, &name, bytes, got);
        }
        Err(reason) => fail(s, gist_error(&name, &link, reason)),
    }
}

/// Writes `text` to `cache` (staged, then renamed in), making its folder.
fn write_cache(cache: &Path, text: &[u8]) -> Result<(), String> {
    if let Some(dir) = cache.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot make {}: {e}", dir.display()))?;
    }
    let tmp = staged(cache, text)?;
    std::fs::rename(&tmp, cache).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot write {}: {e}", cache.display())
    })
}

/// Where the downloaded text of `link` is cached (keyed by loader and path): a gist
/// (`git:<user>/<id>`, user and id ASCII letters, digits, `_` and `-`) at
/// `<data_dir>/gist-cache/<user>_<id>`; a link of another loader CPM has (`gh`, `p`, `pb`, `ms`) at
/// `<data_dir>/link-cache/<loader>/<path>`, every byte of the path but ASCII letters, digits, `-`
/// and `_` written `~XX` (hex). `None` for anything else, or a path that would be over 160 bytes.
pub fn cache_path(data_dir: &Path, link: &str) -> Option<PathBuf> {
    let (loader, path) = link.split_once(':')?;
    if loader == "git" {
        let (user, id) = path.split_once('/')?;
        let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        return (ok(user) && ok(id)).then(|| data_dir.join("gist-cache").join(format!("{user}_{id}")));
    }
    if !["gh", "p", "pb", "ms"].contains(&loader) || cpm_core::link::link_urls(link).is_err() {
        return None;
    }
    let mut file = String::new();
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
            file.push(b as char);
        } else {
            file.push_str(&format!("~{b:02X}"));
        }
    }
    (file.len() <= 160).then(|| data_dir.join("link-cache").join(loader).join(file))
}

/// Sets the mod's data folder (`game.native_data_dir("cpm")`), where the gist cache is kept.
pub fn set_data_dir(dir: PathBuf) {
    STATE.with_borrow_mut(|s| s.data_dir = Some(dir));
}

// ---- Gist downloads: gist_fetch.rs (`game::cpm_gist_fetch`) runs them on threads of their own;
// requests and results cross through a process-wide `Mutex`, not `State` ----

struct Fetches {
    /// Links being downloaded (one download per link).
    in_flight: Vec<String>,
    /// Downloads asked for and not yet started: (link, its URLs in the order tried).
    requests: Vec<(String, Vec<String>)>,
    /// Ended downloads not yet taken: (link, the gist text or why not).
    done: Vec<(String, Result<Vec<u8>, String>)>,
}

static FETCHES: std::sync::Mutex<Fetches> =
    std::sync::Mutex::new(Fetches { in_flight: Vec::new(), requests: Vec::new(), done: Vec::new() });

fn fetches<R>(f: impl FnOnce(&mut Fetches) -> R) -> R {
    // A panic while holding it leaves plain data: keep going with it.
    f(&mut FETCHES.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}

/// Asks for `link`'s text to be downloaded ([`take_fetch_requests`]); false (nothing changes)
/// when it is already downloading, its text is waiting to be taken, or it is not a link CPM can
/// download (`cpm_core::link::link_urls`). An earlier failure not yet taken is dropped: this is a
/// new try.
pub fn start_fetch(link: &str) -> bool {
    let Ok(urls) = cpm_core::link::link_urls(link) else { return false };
    fetches(|f| {
        if f.in_flight.iter().any(|l| l == link) || f.done.iter().any(|(l, r)| l == link && r.is_ok()) {
            return false;
        }
        f.done.retain(|(l, _)| l != link);
        f.in_flight.push(link.to_owned());
        f.requests.push((link.to_owned(), urls));
        true
    })
}

/// The downloads asked for since the last call, (link, its URLs in the order tried): the frame
/// hook starts them.
pub fn take_fetch_requests() -> Vec<(String, Vec<String>)> {
    fetches(|f| std::mem::take(&mut f.requests))
}

/// `link`'s download ended (from any thread) with its gist text, or why not.
pub fn finish_fetch(link: &str, result: Result<Vec<u8>, String>) {
    fetches(|f| {
        f.in_flight.retain(|l| l != link);
        f.done.retain(|(l, _)| l != link);
        f.done.push((link.to_owned(), result));
    });
}

/// Whether `link` is downloading. Only the tests ask (the game just polls in [`ensure_loaded`]).
#[allow(dead_code)]
pub fn fetch_in_flight(link: &str) -> bool {
    fetches(|f| f.in_flight.iter().any(|l| l == link))
}

/// `link`'s ended download, once.
fn take_fetch_result(link: &str) -> Option<Result<Vec<u8>, String>> {
    fetches(|f| {
        let at = f.done.iter().position(|(l, _)| l == link)?;
        Some(f.done.remove(at).1)
    })
}

fn fail(s: &mut State, error: String) {
    log::warn!("{error}");
    s.model = None;
    s.error = Some(error);
}

/// The game would not take the texture: logs `error` and drops the model until the selection changes.
pub fn refuse(error: String) {
    STATE.with_borrow_mut(|s| fail(s, error));
}

/// A model is drawn only with its own texture (no player-skin fallback yet):
/// `cpm: <file>: model has no texture` otherwise.
pub fn check_texture(name: &str, model: &LoadedModel) -> Result<(), String> {
    let [w, h] = model.png_px;
    if w == 0 || h == 0 || model.texture_rgba.is_empty() {
        return Err(format!("cpm: {name}: model has no texture"));
    }
    Ok(())
}

/// Whether a model is loaded (and not refused).
pub fn has_model() -> bool {
    STATE.with_borrow(|s| s.model.is_some())
}

/// The selected file's name, for messages; "" when none.
pub fn selected_name() -> String {
    STATE.with_borrow(|s| shown_name(effective(s).as_deref()))
}

fn file_name(path: Option<&Path>) -> String {
    path.and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// The first time it is called (on this thread): selects what `read` gives, the model the
/// `model` setting keeps (see [`selection_from_setting`]). Later calls do nothing, not even `read`.
pub fn restore_once(read: impl FnOnce() -> Option<PathBuf>) {
    if !STATE.with_borrow_mut(|s| std::mem::replace(&mut s.started, true)) {
        select(read());
    }
}

/// The selected model draws but was made by an older version of the converter: the settings page
/// says to convert its project again (not an error).
pub fn old_conversion() -> bool {
    STATE.with_borrow(|s| s.old_conversion && s.model.is_some())
}

/// Why the selected model is not drawn, if it failed.
pub fn last_error() -> Option<String> {
    STATE.with_borrow(|s| s.error.clone())
}

/// Where the parts of the uploaded image sit in [`SPRITE`], in pixels: the model's texture at
/// the top left, the white patch to its right, then the model's elytra sheet and its cape sheet
/// (each if it has one), in a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Composite {
    /// The whole image's size.
    pub size: [u32; 2],
    /// The white patch's top left ([`WHITE_PX`] a side).
    pub white: [u32; 2],
    /// The elytra sheet's top left, when there is one.
    pub elytra: Option<[u32; 2]>,
    /// The cape sheet's top left, when there is one.
    pub cape: Option<[u32; 2]>,
}

/// The [`Composite`] of a texture `png` pixels in size, an elytra sheet `elytra` and a cape sheet
/// `cape` in size.
pub fn composite(png: [u32; 2], elytra: Option<[u32; 2]>, cape: Option<[u32; 2]>) -> Composite {
    let [pw, ph] = png;
    let at = [pw + WHITE_PX, 0];
    let [ew, eh] = elytra.unwrap_or([0, 0]);
    let cape_at = [at[0] + ew, 0];
    let [cw, ch] = cape.unwrap_or([0, 0]);
    Composite {
        size: [cape_at[0] + cw, ph.max(WHITE_PX).max(eh).max(ch)],
        white: [pw, 0],
        elytra: elytra.map(|_| at),
        cape: cape.map(|_| cape_at),
    }
}

/// A sheet that is usable: a decoded image of its stated size.
fn usable(sheet: &Option<Sheet>) -> Option<&Sheet> {
    sheet.as_ref().filter(|e| {
        let [w, h] = e.png_px;
        w > 0 && h > 0 && e.texture_px[0] > 0 && e.texture_px[1] > 0 && e.rgba.len() == (w * h * 4) as usize
    })
}

/// The model's elytra sheet, when it has a usable one (a decoded image of its stated size).
pub fn elytra_sheet(model: &LoadedModel) -> Option<&Sheet> {
    usable(&model.elytra)
}

/// The model's cape sheet, when it has a usable one (a decoded image of its stated size).
pub fn cape_sheet(model: &LoadedModel) -> Option<&Sheet> {
    usable(&model.cape)
}

fn model_composite(model: &LoadedModel) -> Composite {
    composite(model.png_px, elytra_sheet(model).map(|e| e.png_px), cape_sheet(model).map(|e| e.png_px))
}

/// Copies `rgba` (`size` pixels) into `out` (`out_w` pixels wide) at `at`.
fn blit(out: &mut [u8], out_w: u32, at: [u32; 2], rgba: &[u8], size: [u32; 2]) {
    let row = (size[0] * 4) as usize;
    for y in 0..size[1] as usize {
        let to = ((at[1] as usize + y) * out_w as usize + at[0] as usize) * 4;
        out[to..to + row].copy_from_slice(&rgba[y * row..][..row]);
    }
}

/// The image last made for [`SPRITE`] and where its parts are.
struct Composed {
    size: [u32; 2],
    rgba: Vec<u8>,
    layout: SlotLayout,
    /// Per picker file, where its icon is (`None`: no icon, or no room: a name tile).
    icons: Vec<Option<[u32; 2]>>,
}

/// The image changed (the picker opened or closed, its preview or icons came): make it again and
/// send it at the next frame. The selected model's part stays where it is, so its world draw goes
/// on meanwhile.
fn recompose(s: &mut State) {
    s.composed = None;
    s.upload_due = true;
}

/// The image to upload into [`SPRITE`] (`width`, `height`, RGBA): when its contents changed (a
/// model was just loaded, the picker's preview or icons changed), or [`REUPLOAD`] after the last
/// upload. The selected model's part is laid out as [`composite`] says (refused, an error and the
/// model dropped, when that is wider or taller than the sprite); with the picker open the
/// previewed model's image and the icons follow as [`slot_layout`] places them. Made again only
/// when its contents change; once the picker closes the image is the selected model's alone again
/// (the picker's parts left in the sprite past it are never sampled).
pub fn texture_upload(now: Instant) -> Option<(u32, u32, Vec<u8>)> {
    STATE.with_borrow_mut(|s| {
        if s.model.is_none() && s.picker.is_none() {
            s.composed = None;
            return None;
        }
        if s.model.is_some() && s.uploaded.is_none() {
            // A model just loaded.
            s.composed = None;
        }
        let due = s.upload_due
            || s.composed.is_none()
            || s.last_upload.is_none_or(|t| now.saturating_duration_since(t) >= REUPLOAD);
        if !due {
            return None;
        }
        if s.composed.is_none() {
            s.composed = Some(compose(s)?);
        }
        s.upload_due = false;
        s.last_upload = Some(now);
        if s.model.is_some() {
            s.uploaded = Some(now);
        }
        let c = s.composed.as_ref()?;
        // Nothing placed yet (the picker open with no model, its reads not back): nothing to send.
        if c.size[0] == 0 || c.size[1] == 0 {
            return None;
        }
        Some((c.size[0], c.size[1], c.rgba.clone()))
    })
}

/// Makes the image: the selected model's part (`None`, the model refused, when it is too big),
/// then the picker's.
fn compose(s: &mut State) -> Option<Composed> {
    let mut active = [0, 0];
    if let Some(model) = s.model.as_ref() {
        let layout = model_composite(model);
        let [w, h] = layout.size;
        if w > SPRITE_PX || h > SPRITE_PX {
            let name = shown_name(s.loaded.as_deref());
            let [pw, ph] = model.png_px;
            let sheet = |s: Option<&Sheet>, what: &str| {
                s.map(|e| format!(" with a {}x{} {what} sheet", e.png_px[0], e.png_px[1])).unwrap_or_default()
            };
            let error = format!(
                "cpm: {name}: texture {pw}x{ph}{}{} is too big (at most {}x{SPRITE_PX} in all)",
                sheet(elytra_sheet(model), "elytra"),
                sheet(cape_sheet(model), "cape"),
                SPRITE_PX - WHITE_PX
            );
            fail(s, error);
            return None;
        }
        active = layout.size;
    }
    let picker = s.picker.as_ref();
    let preview = picker.and_then(|p| match &p.preview {
        Preview::Ready(m) => Some(&m.model),
        _ => None,
    });
    let icons: Vec<(usize, &Vec<u8>)> = picker
        .map(|p| p.tiles.iter().enumerate().filter_map(|(i, t)| Some((i, t.icon.as_ref()?))).collect())
        .unwrap_or_default();
    let layout = slot_layout(active, preview.map(|m| model_composite(m).size), icons.len());
    // The image's size: whatever was placed.
    let mut size = active;
    let mut grow = |at: [u32; 2], px: [u32; 2]| size = [size[0].max(at[0] + px[0]), size[1].max(at[1] + px[1])];
    if let (Some(at), Some(m)) = (layout.preview_at, preview) {
        grow(at, model_composite(m).size);
    }
    for at in layout.icon_at.iter().flatten() {
        grow(*at, [ICON_PX, ICON_PX]);
    }
    let w = size[0];
    let mut rgba = vec![0u8; (size[0] * size[1] * 4) as usize];
    if let Some(model) = s.model.as_ref() {
        put_model(&mut rgba, w, model, [0, 0]);
    }
    if let (Some(at), Some(m)) = (layout.preview_at, preview) {
        put_model(&mut rgba, w, m, at);
    }
    let mut placed = vec![None; picker.map_or(0, |p| p.files.len())];
    for ((i, icon), at) in icons.iter().zip(&layout.icon_at) {
        if let Some(at) = at {
            blit(&mut rgba, w, *at, icon, [ICON_PX, ICON_PX]);
            if let Some(p) = placed.get_mut(*i) {
                *p = Some(*at);
            }
        }
    }
    Some(Composed { size, rgba, layout, icons: placed })
}

/// Puts `model`'s part of the image ([`composite`]) into `rgba` (`w` pixels wide) at `base`.
fn put_model(rgba: &mut [u8], w: u32, model: &LoadedModel, base: [u32; 2]) {
    let c = model_composite(model);
    blit(rgba, w, base, &model.texture_rgba, model.png_px);
    for y in 0..WHITE_PX {
        let at = (((base[1] + y) * w + base[0] + c.white[0]) * 4) as usize;
        rgba[at..at + (WHITE_PX * 4) as usize].fill(255);
    }
    if let (Some(at), Some(e)) = (c.elytra, elytra_sheet(model)) {
        blit(rgba, w, add2(base, at), &e.rgba, e.png_px);
    }
    if let (Some(at), Some(e)) = (c.cape, cape_sheet(model)) {
        blit(rgba, w, add2(base, at), &e.rgba, e.png_px);
    }
}

/// Puts `model`'s part into the image `c` at `base` when it fits there; false (nothing written)
/// when it does not (the image was made for another model).
fn patch(c: &mut Composed, model: &LoadedModel, base: [u32; 2]) -> bool {
    let [pw, ph] = model_composite(model).size;
    let fits = base[0] + pw <= c.size[0] && base[1] + ph <= c.size[1] && c.rgba.len() == (c.size[0] * c.size[1] * 4) as usize;
    if fits {
        put_model(&mut c.rgba, c.size[0], model, base);
    }
    fits
}

/// Runs the animated textures ([`LoadedModel::tick_textures`]) of the selected model and of the
/// picker's previewed one at the animation clock ([`clock_ms`]) `now`. A model whose pixels
/// changed has its part of the image put in again, and the image is sent at the next
/// [`texture_upload`]; otherwise nothing is sent.
pub fn tick_textures(now: Instant) {
    let time = clock_ms(now);
    STATE.with_borrow_mut(|s| {
        let model = s.model.as_mut().is_some_and(|m| m.tick_textures(time));
        let preview = match s.picker.as_mut().map(|p| &mut p.preview) {
            Some(Preview::Ready(p)) => p.model.tick_textures(time),
            _ => false,
        };
        if !model && !preview {
            return;
        }
        s.upload_due = true;
        // Only an image made for the model shown now is patched (a model that has not reached
        // the game yet gets a new one at the next upload anyway); else it is made again.
        let current = s.model.is_none() || s.uploaded.is_some();
        let Some(c) = s.composed.as_mut().filter(|_| current) else {
            s.composed = None;
            return;
        };
        let mut patched = true;
        if model && let Some(m) = s.model.as_ref() {
            patched &= patch(c, m, [0, 0]);
        }
        if preview
            && let (Some(at), Some(Picker { preview: Preview::Ready(p), .. })) = (c.layout.preview_at, s.picker.as_ref())
        {
            patched &= patch(c, &p.model, at);
        }
        if !patched {
            s.composed = None;
        }
    })
}

/// Runs `f` on the loaded model; `None` when there is none.
pub fn with_model<R>(f: impl FnOnce(&mut LoadedModel) -> R) -> Option<R> {
    STATE.with_borrow_mut(|s| s.model.as_mut().map(f))
}

/// Pixels of [`SPRITE`] to its texels (16 a side), as `native_draw_quad` takes them.
pub fn uv_to_sprite(px: [f32; 4]) -> [f32; 4] {
    px.map(|v| v * 16.0 / SPRITE_PX as f32)
}

/// A face's `uv` (pixels of the declared size `texture_px` of an image placed at `at` in
/// [`SPRITE`]: the model's texture at (0, 0), its elytra sheet at [`Composite::elytra`]) in
/// sprite texels: scaled to the image's real size `png_px` first, as CPM stretches the PNG over
/// the declared size.
pub fn slot_uv(uv: [f32; 4], texture_px: [u32; 2], png_px: [u32; 2], at: [u32; 2]) -> [f32; 4] {
    let scale = |png: u32, declared: u32| if declared == 0 { 1.0 } else { png as f32 / declared as f32 };
    let (sx, sy) = (scale(png_px[0], texture_px[0]), scale(png_px[1], texture_px[1]));
    let (x, y) = (at[0] as f32, at[1] as f32);
    uv_to_sprite([x + uv[0] * sx, y + uv[1] * sy, x + uv[2] * sx, y + uv[3] * sy])
}

/// A face's `uv` (pixels of a `texture_px` texture) on a sprite of its own (an armour or elytra
/// texture of the game's atlas): its fraction of the texture, 16 texels a side, as
/// `native_draw_quad` takes it.
pub fn own_uv(uv: [f32; 4], texture_px: [u32; 2]) -> [f32; 4] {
    let d = |v: u32| if v == 0 { 1.0 } else { v as f32 };
    let (w, h) = (d(texture_px[0]), d(texture_px[1]));
    [uv[0] / w * 16.0, uv[1] / h * 16.0, uv[2] / w * 16.0, uv[3] / h * 16.0]
}

/// The white patch's centre, for every corner of an untextured face.
/// Only the tests ask (the draws take the patch where the image is: [`white_uv_at`]).
#[allow(dead_code)]
pub fn white_uv(png_w: u32) -> [f32; 4] {
    white_uv_at([png_w, 0])
}

/// The centre of a white patch at `at` in [`SPRITE`], for every corner.
fn white_uv_at(at: [u32; 2]) -> [f32; 4] {
    let [u, v, ..] = uv_to_sprite([(at[0] + WHITE_PX / 2) as f32, (at[1] + WHITE_PX / 2) as f32, 0.0, 0.0]);
    [u, v, u, v]
}

fn add2(a: [u32; 2], b: [u32; 2]) -> [u32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

/// One face to draw with `native_draw_quad`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    /// Blocks, TL, BL, BR, TR: for [`quads`] in the player's draw frame (feet origin, Y up, +Z
    /// facing, +X its left); for [`arm_quads`] in the first-person arm's model space (Y down),
    /// which the game's arm pose takes.
    pub corners: [[f32; 3]; 4],
    /// Texels (16 a side) of its sprite.
    pub uv: [f32; 4],
    /// 0xRRGGBB.
    pub tint: u32,
    /// Drawn at [`FULL_BRIGHT`] (CPM's glow: the additive pass when untinted, [`split_glow`]).
    pub glow: bool,
    /// An OptiFine emissive overlay ([`WornLayer::emissive`]): drawn at [`FULL_BRIGHT`] as an
    /// ordinary face, never in the glow pass.
    pub emissive: bool,
    /// The armour glint (`mobs::render::ARMOR_GLINT`), with the face's place in its own texture
    /// (`[u0, v0, u1, v1]`, 0 to 1: the model UV the shader scrolls the glint by).
    pub glint: Option<[f32; 4]>,
    /// Index into [`Quads::sprites`] of the sprite it samples (0: [`SPRITE`]).
    pub sprite: usize,
}

/// A glint rect's UV at each of a quad's corners, in `native_draw_quad`'s corner order (TL, BL,
/// BR, TR: `[u0, v0]`, `[u0, v1]`, `[u1, v1]`, `[u1, v0]`), for the vertices' `glint_uv`.
pub fn glint_corners([u0, v0, u1, v1]: [f32; 4]) -> [[f32; 2]; 4] {
    [[u0, v0], [u0, v1], [u1, v1], [u1, v0]]
}

/// Orders `quads` as Minecraft draws an `entityTranslucent` buffer, which CPM draws models with
/// (`RenderTypes.entityTranslucent`, sorted on upload by `VertexSorting.DISTANCE_TO_ORIGIN`):
/// farthest first by the camera distance of each quad's centre, a stable sort so ties keep their
/// order. `pose` takes the quads' frame to the camera-relative draw space (the camera at the
/// origin). Horizon's entity pass draws in submission order with no culling and lets a later face
/// win a depth tie, as vanilla does, so a model's faces that lie on one another (layered faces,
/// flat boxes) show the one CPM shows.
pub fn back_to_front(quads: &mut [Quad], pose: Mat4) {
    let dist = |q: &Quad| {
        let c = q.corners.iter().fold(Vec3::ZERO, |s, p| s + Vec3::from_array(*p)) / 4.0;
        pose.transform_point3(c).length_squared()
    };
    let mut keyed: Vec<(f32, Quad)> = quads.iter().map(|q| (dist(q), q.clone())).collect();
    keyed.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (slot, (_, q)) in quads.iter_mut().zip(keyed) {
        *slot = q;
    }
}

/// Whether `q` faces the camera under `pose` (the camera at the origin of the draw space): its
/// outward normal (`(BL - TL) × (BR - TL)`) points towards the camera.
pub fn facing_camera(q: &Quad, pose: Mat4) -> bool {
    let p = q.corners.map(|c| pose.transform_point3(Vec3::from_array(c)));
    (p[1] - p[0]).cross(p[2] - p[0]).dot(-p[0]) > 0.0
}

/// Splits `quads` for the world draw as CPM renders them: glowing faces go to its eyes layer
/// (`CustomRenderTypes.EYES`: additive, back faces culled, no depth writes, drawn after the
/// rest), the others to the solid one (`entityTranslucent`, nothing culled). A glowing face turned
/// away from the camera is left out; an untinted one is returned in the second list, for an
/// additive pass; a tinted one (`COLOR_GLOW`) stays with the solid faces, as the additive pass
/// cannot tint. Each list keeps the order of `quads`.
pub fn split_glow(quads: &[Quad], pose: Mat4) -> (Vec<Quad>, Vec<Quad>) {
    let mut solid = Vec::new();
    let mut additive = Vec::new();
    for q in quads {
        if !q.glow {
            solid.push(q.clone());
        } else if facing_camera(q, pose) {
            if q.tint & 0xFF_FFFF == 0xFF_FFFF { additive.push(q.clone()) } else { solid.push(q.clone()) }
        }
    }
    (solid, additive)
}

/// A vertex's atlas `uv` as its place in the sprite `rect` (`[u0, v0, u1, v1]`, atlas UV): the UV
/// Horizon's additive energy swirl pass samples by (`fs_energy_swirl`, `crumbling_uv`).
pub fn swirl_uv(uv: [f32; 2], rect: [f32; 4]) -> [f32; 2] {
    let d = |a: f32, b: f32| if b - a == 0.0 { 1.0 } else { b - a };
    [(uv[0] - rect[0]) / d(rect[0], rect[2]), (uv[1] - rect[1]) / d(rect[1], rect[3])]
}

/// Quads to draw and the sprites they sample.
#[derive(Debug, Clone, PartialEq)]
pub struct Quads {
    /// Atlas sprite names; the first is always [`SPRITE`] (the model's image).
    pub sprites: Vec<String>,
    pub quads: Vec<Quad>,
    /// The held and head items to draw with the model (none for the first-person arm).
    pub items: Vec<ItemDraw>,
    /// Where the shoulder parrots go ([`parrot_draws`]; none for the first-person arm).
    pub shoulders: Vec<ShoulderAnchor>,
}

impl Quads {
    fn new() -> Self {
        Quads { sprites: vec![SPRITE.to_owned()], quads: Vec::new(), items: Vec::new(), shoulders: Vec::new() }
    }

    /// The index of sprite `name`, added when new.
    fn sprite(&mut self, name: &str) -> usize {
        match self.sprites.iter().position(|s| s == name) {
            Some(i) => i,
            None => {
                self.sprites.push(name.to_owned());
                self.sprites.len() - 1
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Held and head items

/// The item model display context an item is drawn in (`ItemDisplayContext`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemDisplay {
    ThirdPersonLeftHand,
    ThirdPersonRightHand,
    Head,
}

/// One held or head item to draw (the game picks the stack by `slot`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemDraw {
    pub slot: ItemSlot,
    pub display: ItemDisplay,
    /// Added to the player's entity id for the item model's seed: the context's ordinal, as
    /// `ItemModelResolver.updateForLiving` does (left hand 1, right hand 2, head 5).
    pub seed: i32,
    /// A worn skull or head, drawn with the skull renderer (`CustomHeadLayer`'s skull path), not
    /// as an item model.
    pub skull: bool,
    /// Item space (centred on the origin, before the display transform; for a skull, the skull
    /// renderer's space) to the player's draw frame (blocks, feet origin, Y up).
    pub transform: Mat4,
}

/// `ItemInHandLayer.submitArmWithItem` after `translateToHand`: turned to point along the arm,
/// then (±1, 2, -10) pixels, + for the right arm.
fn hand_layer(left: bool) -> Mat4 {
    Mat4::from_rotation_x(-90f32.to_radians())
        * Mat4::from_rotation_y(std::f32::consts::PI)
        * Mat4::from_translation(Vec3::new(if left { -1.0 } else { 1.0 } / 16.0, 0.125, -0.625))
}

/// `CustomHeadLayer` for an item (not a skull) after `translateToHead`.
fn head_layer() -> Mat4 {
    Mat4::from_translation(Vec3::new(0.0, -0.25, 0.0))
        * Mat4::from_rotation_y(std::f32::consts::PI)
        * Mat4::from_scale(Vec3::new(0.625, -0.625, -0.625))
}

/// `CustomHeadLayer`'s `SKULL_SCALE`: a worn skull is drawn at 1.1875 after `translateToHead`.
const SKULL_SCALE: f32 = 1.1875;

/// The item drawn at `anchor`: its layer's placement after the anchor and its display context.
/// `skull`: the head slot holds a skull or head (drawn as `CustomHeadLayer` draws skulls).
pub fn item_draw(anchor: &ItemAnchor, skull: bool) -> ItemDraw {
    let skull = skull && anchor.slot == ItemSlot::Head;
    let (display, seed, layer) = match (anchor.slot, anchor.left) {
        (ItemSlot::Head, _) if skull => (ItemDisplay::Head, 5, Mat4::from_scale(Vec3::splat(SKULL_SCALE))),
        (ItemSlot::Head, _) => (ItemDisplay::Head, 5, head_layer()),
        (_, true) => (ItemDisplay::ThirdPersonLeftHand, 1, hand_layer(true)),
        (_, false) => (ItemDisplay::ThirdPersonRightHand, 2, hand_layer(false)),
    };
    ItemDraw { slot: anchor.slot, display, seed, skull, transform: anchor.transform * layer }
}

/// [`item_draw`] for each anchor, leaving out one that flattens the item to nothing (a zero
/// render or mesh scale on its item part): Java draws nothing visible there.
pub fn item_draws(anchors: &[ItemAnchor], skull: bool) -> Vec<ItemDraw> {
    anchors
        .iter()
        .filter(|a| a.transform.determinant().abs() > 1e-9 && a.transform.is_finite())
        .map(|a| item_draw(a, skull))
        .collect()
}

/// The shoulder anchors a parrot is drawn at, leaving out one that flattens it to nothing (a zero
/// render or mesh scale on its shoulder part), as [`item_draws`] does. The game draws a parrot
/// only on a shoulder that has one (parrot_probe.rs).
pub fn parrot_draws(anchors: &[ShoulderAnchor]) -> Vec<ShoulderAnchor> {
    anchors.iter().filter(|a| a.transform.determinant().abs() > 1e-9 && a.transform.is_finite()).copied().collect()
}

// ---------------------------------------------------------------------------------------------
// Worn armour and elytra

/// One texture of a worn armour piece: an atlas sprite drawn over the piece's boxes, times `tint`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WornLayer {
    pub sprite: String,
    /// 0xRRGGBB.
    pub tint: u32,
    /// Drawn with the armour glint (a foil stack's first layer and trim: [`piece_layers`]).
    pub glint: bool,
    /// An OptiFine emissive overlay: drawn at full brightness, without the glint.
    pub emissive: bool,
}

/// One worn armour piece's layers as Horizon's `armor_layers` draws them
/// (`EquipmentLayerRenderer.renderLayers`): each of `layers` in turn, the first with the glint
/// when the stack `foil`s (`hasFoil`), each followed by its OptiFine emissive overlay when it has
/// one (the atlas sprite: full bright, its layer's colour, no glint), then the `trim` (untinted,
/// with the glint when foil). No layers, nothing (not even the trim).
pub fn piece_layers(layers: Vec<(WornLayer, Option<String>)>, trim: Option<String>, foil: bool) -> Vec<WornLayer> {
    let mut out = Vec::new();
    if layers.is_empty() {
        return out;
    }
    for (i, (layer, emissive)) in layers.into_iter().enumerate() {
        let tint = layer.tint;
        out.push(WornLayer { glint: foil && i == 0, emissive: false, ..layer });
        if let Some(sprite) = emissive {
            out.push(WornLayer { sprite, tint, glint: false, emissive: true });
        }
    }
    if let Some(sprite) = trim {
        out.push(WornLayer { sprite, tint: 0xFF_FFFF, glint: foil, emissive: false });
    }
    out
}

/// What the local player wears, as the model draws it (the game side fills it:
/// equipment_probe.rs). A slot with no layers is drawn as not worn.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Worn {
    /// Helmet, chestplate, leggings, boots ([`slot_index`]): each piece's layers, in draw order
    /// (the base texture, leather's overlay, a trim).
    pub armor: [Vec<WornLayer>; 4],
    /// An elytra is in the chest slot.
    pub elytra: bool,
    /// Its texture in the atlas, when the atlas has one (a CIT rule's, the player's cape, else the
    /// elytra's own).
    pub elytra_sprite: Option<String>,
    /// The worn elytra `hasFoil()`: its wings glint (`WingsLayer` through `renderLayers`), on
    /// whatever texture they take.
    pub elytra_glint: bool,
    /// The elytra texture's OptiFine emissive overlay in the atlas (none when the wings take the
    /// player's cape), drawn over wings in [`Worn::elytra_sprite`] at full brightness.
    pub elytra_emissive: Option<String>,
    /// A skull or head is in the head slot ([`Equipment::skull`]).
    pub skull: bool,
    /// The player's cape in the atlas, when it has one the atlas holds.
    pub cape_sprite: Option<String>,
    /// The chest item has a `wings` equipment layer ([`Equipment::chest_has_wings`]).
    pub chest_has_wings: bool,
    /// The chest item has a `humanoid` equipment layer ([`Equipment::chest_humanoid`]).
    pub chest_humanoid: bool,
}

impl Worn {
    /// The model's [`Equipment`]: a slot with layers is worn; the elytra when the model has its
    /// own sheet (`model_elytra`) or the worn one has a texture.
    pub fn equipment(&self, model_elytra: bool) -> Equipment {
        let has = |i: usize| !self.armor[i].is_empty();
        Equipment {
            head: has(0),
            chest: has(1),
            legs: has(2),
            feet: has(3),
            elytra: self.elytra && (model_elytra || self.elytra_sprite.is_some()),
            skull: self.skull,
            chest_has_wings: self.chest_has_wings,
            chest_humanoid: self.chest_humanoid,
        }
    }
}

/// `slot`'s index in [`Worn::armor`] (and in the game's armour slots, head to feet).
pub fn slot_index(slot: ArmorSlot) -> usize {
    match slot {
        ArmorSlot::Helmet => 0,
        ArmorSlot::Chestplate => 1,
        ArmorSlot::Leggings => 2,
        ArmorSlot::Boots => 3,
    }
}

/// The armour slots in [`Worn::armor`] order.
pub const ARMOR_SLOTS: [ArmorSlot; 4] = [ArmorSlot::Helmet, ArmorSlot::Chestplate, ArmorSlot::Leggings, ArmorSlot::Boots];

/// The equipment layer kind of `slot`: `humanoid_leggings` for the leggings, `humanoid` else.
pub fn armor_kind(slot: ArmorSlot) -> &'static str {
    if slot == ArmorSlot::Leggings { "humanoid_leggings" } else { "humanoid" }
}

/// Leather armour's colour when undyed (the `leather` equipment asset's `color_when_undyed`).
pub const LEATHER_UNDYED: u32 = 0xA0_6540;

/// The worn elytra's texture (the `minecraft:elytra` equipment asset's `wings` layer).
pub const ELYTRA_SPRITE: &str = "entity/equipment/wings/elytra";

/// Whether `item` is the vanilla elytra.
pub fn is_elytra(item: &str) -> bool {
    item == "minecraft:elytra"
}

/// The equipment asset of the vanilla armour item `item` worn in `slot`, as Horizon names it
/// (`mobs/render.rs` `humanoid_armor`): `minecraft:<material>_<helmet|chestplate|leggings|boots>`
/// gives `<material>`, `golden` as `gold` and `turtle` as `turtle_scute`. `None` for anything else
/// (another namespace, a pumpkin, an elytra, another slot's piece): that slot draws nothing.
pub fn armor_asset(item: &str, slot: ArmorSlot) -> Option<&str> {
    let suffix = match slot {
        ArmorSlot::Helmet => "_helmet",
        ArmorSlot::Chestplate => "_chestplate",
        ArmorSlot::Leggings => "_leggings",
        ArmorSlot::Boots => "_boots",
    };
    let material = item.strip_prefix("minecraft:")?.strip_suffix(suffix)?;
    if material.is_empty() {
        return None;
    }
    Some(match material {
        "golden" => "gold",
        "turtle" => "turtle_scute",
        other => other,
    })
}

/// The layers of the vanilla armour item `item` worn in `slot` (`dyed`, 0xRRGGBB, the stack's dye
/// when it has one): `entity/equipment/<kind>/<asset>`; for leather that layer tinted with the dye
/// (else [`LEATHER_UNDYED`]), then an untinted `…/leather_overlay`. Empty for an unknown item.
pub fn armor_layers(item: &str, slot: ArmorSlot, dyed: Option<u32>) -> Vec<WornLayer> {
    let Some(asset) = armor_asset(item, slot) else {
        return Vec::new();
    };
    let kind = armor_kind(slot);
    let base = format!("entity/equipment/{kind}/{asset}");
    if asset != "leather" {
        return vec![WornLayer { sprite: base, tint: 0xFF_FFFF, ..Default::default() }];
    }
    vec![
        WornLayer { sprite: base, tint: dyed.unwrap_or(LEATHER_UNDYED) & 0xFF_FFFF, ..Default::default() },
        WornLayer { sprite: format!("entity/equipment/{kind}/leather_overlay"), tint: 0xFF_FFFF, ..Default::default() },
    ]
}

/// Two 0xRRGGBB colours multiplied.
pub fn mul_rgb(a: u32, b: u32) -> u32 {
    let ch = |s: u32| ((a >> s & 255) * (b >> s & 255) + 127) / 255;
    ch(16) << 16 | ch(8) << 8 | ch(0)
}

/// How one pass over a box samples.
#[derive(Clone, Copy)]
enum Sample {
    /// [`SPRITE`]: the image at this pixel position, of this real size.
    Slot([u32; 2], [u32; 2]),
    /// A sprite of its own (index into [`Quads::sprites`]), times a tint.
    Own(usize, u32),
}

/// One pass over a box: how it samples, with the armour glint, as an emissive overlay.
#[derive(Clone, Copy)]
struct Pass {
    sample: Sample,
    glint: bool,
    emissive: bool,
}

impl Pass {
    fn slot(at: [u32; 2], png: [u32; 2], glint: bool) -> Self {
        Pass { sample: Sample::Slot(at, png), glint, emissive: false }
    }

    fn own(sprite: usize, tint: u32, glint: bool) -> Self {
        Pass { sample: Sample::Own(sprite, tint), glint, emissive: false }
    }
}

/// `boxes` (posed from `model`) as quads: each face's corners through its box's transform, its
/// UVs into its texture. Skin boxes and the model's elytra and cape sheets sample [`SPRITE`] (the
/// white patch for an untextured box); armour boxes take each of their slot's [`Worn`] layers in
/// turn, elytra boxes without the model's sheet the worn elytra's sprite, cape boxes without the
/// model's sheet (and the vanilla cape) the player's cape. A box with nothing to sample (a slot
/// with no layers, a cape with no texture) is left out.
pub fn box_quads(model: &LoadedModel, boxes: &[PosedBox], worn: &Worn) -> Quads {
    let mut out = Quads::new();
    let mut passes = Vec::new();
    for b in boxes {
        box_passes(model, b, worn, [0, 0], &mut out, &mut passes);
        for &pass in &passes {
            for face in b.faces.iter().flatten() {
                let corners = face.corners.map(|c| b.transform.transform_point3(c.into()).to_array());
                let (uv, sprite, tint) = pass.map(model, b, face, [0, 0]);
                // The glint scrolls by the face's place in its own texture. None on a glowing box
                // (CPM's RenderMode.GLOW draws in its own render type, never the glint's).
                let glint = (pass.glint && !b.glow).then(|| own_uv(face.uv, b.texture_px).map(|t| t / 16.0));
                out.quads.push(Quad { corners, uv, tint, glow: b.glow && !pass.emissive, emissive: pass.emissive, glint, sprite });
            }
        }
    }
    out
}

/// The passes box `b` is drawn in (into `passes`, cleared first), adding the sprites they sample
/// to `out`: skin boxes and the model's elytra sheet sample [`SPRITE`] (an untextured box the
/// white patch); armour boxes take each of their slot's [`Worn`] layers in turn (with their glint
/// and emissive overlays), elytra boxes without the model's sheet the worn elytra's sprite and its
/// emissive overlay. An enchanted elytra's textured boxes glint on either (colour cubes never do).
/// None for a box with nothing to sample.
/// `base`: where the model's image ([`composite`]) is in [`SPRITE`] (the selected model's at the
/// top left, the picker's previewed one where [`slot_layout`] put it).
fn box_passes(model: &LoadedModel, b: &PosedBox, worn: &Worn, base: [u32; 2], out: &mut Quads, passes: &mut Vec<Pass>) {
    passes.clear();
    match b.texture {
        // A colour cube: the white patch times its colour, whatever it is worn on. Never the glint:
        // CPM draws it as RenderMode.COLOR, not in the armour's (glint) render type.
        _ if !b.textured => passes.push(Pass::slot(base, model.png_px, false)),
        BoxTexture::Skin => passes.push(Pass::slot(base, model.png_px, false)),
        BoxTexture::Armor(slot) => {
            for l in &worn.armor[slot_index(slot)] {
                let pass = Pass::own(out.sprite(&l.sprite), l.tint, l.glint && !l.emissive);
                passes.push(Pass { emissive: l.emissive, ..pass });
            }
        }
        // WingsLayer: the glint on any texture (CPM only swaps it); the emissive overlay only
        // over the elytra's own texture (OptiFine finds none for the model's).
        BoxTexture::Elytra => match (elytra_sheet(model), model_composite(model).elytra, &worn.elytra_sprite) {
            (Some(e), Some(at), _) => passes.push(Pass::slot(add2(base, at), e.png_px, worn.elytra_glint)),
            (_, _, Some(sprite)) => {
                passes.push(Pass::own(out.sprite(sprite), 0xFF_FFFF, worn.elytra_glint));
                if let Some(e) = &worn.elytra_emissive {
                    passes.push(Pass { emissive: true, ..Pass::own(out.sprite(e), 0xFF_FFFF, false) });
                }
            }
            _ => {}
        },
        // CapeLayerMixin: the model's cape sheet, else the player's cape; Java's template cape
        // (no cape at all) is not shipped, so nothing.
        BoxTexture::Cape => match (cape_sheet(model), model_composite(model).cape, &worn.cape_sprite) {
            (Some(e), Some(at), _) => passes.push(Pass::slot(add2(base, at), e.png_px, false)),
            (_, _, Some(sprite)) => passes.push(Pass::own(out.sprite(sprite), 0xFF_FFFF, false)),
            _ => {}
        },
        // Vanilla's CapeLayer: the player's cape only.
        BoxTexture::PlayerCape => {
            if let Some(sprite) = &worn.cape_sprite {
                passes.push(Pass::own(out.sprite(sprite), 0xFF_FFFF, false));
            }
        }
    }
}

impl Pass {
    /// Face `face` of box `b` in this pass: its UV (sprite texels), sprite index and 0xRRGGBB tint
    /// (`base` as for [`box_passes`]).
    fn map(self, model: &LoadedModel, b: &PosedBox, face: &cpm_core::FaceQuad, base: [u32; 2]) -> ([f32; 4], usize, u32) {
        match self.sample {
            _ if !b.textured => (white_uv_at(add2(base, model_composite(model).white)), 0, b.tint),
            Sample::Slot(at, png) => (slot_uv(face.uv, b.texture_px, png, at), 0, b.tint),
            Sample::Own(i, tint) => (own_uv(face.uv, b.texture_px), i, mul_rgb(b.tint, tint)),
        }
    }
}

/// The loaded model's quads from `pose`; `None` when no model is loaded or its texture has not
/// reached the game yet.
fn posed_quads(worn: &Worn, pose: impl FnOnce(&mut LoadedModel) -> Posed) -> Option<Quads> {
    if STATE.with_borrow(|s| s.uploaded.is_none()) {
        return None;
    }
    with_model(|m| {
        let posed = pose(m);
        let mut quads = box_quads(m, &posed.boxes, worn);
        quads.items = item_draws(&posed.items, worn.skull);
        quads.shoulders = parrot_draws(&posed.shoulders);
        quads
    })
}

/// The loaded model posed for `input` wearing `worn` (the equipment comes from `worn`, not
/// `input`), as quads with its held and head items ([`item_draw`]); `None` when no model is
/// loaded or its texture has not reached the game yet.
pub fn quads(input: &PoseInput, worn: &Worn) -> Option<Quads> {
    posed_quads(worn, |m| {
        let input = PoseInput { equipment: worn.equipment(elytra_sheet(m).is_some()), ..*input };
        m.pose_with_items(&input)
    })
}

/// The model's first-person arm (`left`, else the right one: [`LoadedModel::pose_arm`]) as
/// quads; `None` as for [`quads`]. It wears nothing, as vanilla's first-person arm does not.
pub fn arm_quads(left: bool) -> Option<Quads> {
    posed_quads(&Worn::default(), |m| Posed { boxes: m.pose_arm(left), items: Vec::new(), shoulders: Vec::new() })
}

/// The model's head as a player head shows it ([`LoadedModel::pose_skull`], the skull's poses at
/// `time_ms`: [`clock_ms`]) as quads in the skull `head` part's frame (blocks, Y down); `None` as
/// for [`quads`]. It wears nothing.
pub fn skull_quads(time_ms: u64) -> Option<Quads> {
    posed_quads(&Worn::default(), |m| Posed { boxes: m.pose_skull(time_ms), items: Vec::new(), shoulders: Vec::new() })
}

// ---- Player heads: whose they are. head_probe.rs reads the local player's identities from the
// game and a head's profile from its `minecraft:profile`; the drawing asks here.

/// One identity of the local player: a UUID (none when not known) and a name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    pub id: Option<u128>,
    pub name: String,
}

/// What a player head's profile names (`ResolvableProfile`): its UUID and its name, each when
/// given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HeadProfile {
    pub id: Option<u128>,
    pub name: Option<String>,
}

/// True when `head` is one of `own` (`CustomPlayerModelsClient.manager.loadSkull` resolves the
/// profile to a player, and only the local player has a model until multiplayer): by UUID when the
/// head has one, else by name (Minecraft names compare without case). A head naming nothing
/// matches nothing.
pub fn profile_matches(head: &HeadProfile, own: &[Identity]) -> bool {
    match (head.id, head.name.as_deref()) {
        (Some(id), _) => own.iter().any(|o| o.id == Some(id)),
        (None, Some(name)) => !name.is_empty() && own.iter().any(|o| o.name.eq_ignore_ascii_case(name)),
        (None, None) => false,
    }
}

/// Sets the local player's identities (none outside a world or while the mod is off: no head is
/// then the player's).
pub fn set_own_identities(own: Vec<Identity>) {
    STATE.with_borrow_mut(|s| s.own = own);
}

/// True when `head` is the local player's ([`profile_matches`] against [`set_own_identities`]).
pub fn is_own_head(head: &HeadProfile) -> bool {
    STATE.with_borrow(|s| profile_matches(head, &s.own))
}

/// Milliseconds since the first call: the animation clock.
pub fn clock_ms(now: Instant) -> u64 {
    STATE.with_borrow_mut(|s| {
        let start = *s.start.get_or_insert(now);
        now.saturating_duration_since(start).as_millis() as u64
    })
}

// ---- Action, hurt and dying state: action_probe.rs reads it from the game, player_submit.rs
// puts it into the pose input here.

/// The pull an item in use reports (`PlayerProfile.updateFromPlayer`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pull {
    #[default]
    None,
    /// A bow (`BowItem`): `bowPullback` over 20 ticks.
    Bow,
    /// A crossbow (`CrossbowItem`) and its charge duration in ticks: `crossbowPullback`.
    Crossbow(u32),
}

/// The item in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Use {
    pub hand: cpm_core::Hand,
    /// Its use animation; `None` for one CPM has no pose for (none, bundle).
    pub kind: Option<cpm_core::UseKind>,
    /// Ticks it has been used (`getTicksUsingItem`).
    pub ticks: u32,
    pub pull: Pull,
}

/// The local player's state that CPM's action poses read, in plain values.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ActionState {
    /// The swing's hand and animation (`getCurrentSwing`, `swingAnimation`); `None` when not
    /// swinging.
    pub swing: Option<(cpm_core::Hand, f32)>,
    /// A swing is under way (`swinging`): a charged crossbow is not held up meanwhile.
    pub swinging: bool,
    pub using: Option<Use>,
    /// A stack in the main / off hand.
    pub holding: [bool; 2],
    /// A charged crossbow in the main / off hand.
    pub charged: [bool; 2],
    /// `onClimbable`.
    pub on_climbable: bool,
    /// This tick's vertical movement (`getY() - yo`).
    pub dy: f64,
    /// `isVisuallySwimming`.
    pub visually_swimming: bool,
    /// `isInWater`.
    pub in_water: bool,
    /// `Pose.SPIN_ATTACK` (riptide).
    pub spin_attack: bool,
    /// `getTicksFrozen`.
    pub ticks_frozen: i32,
    pub health: f32,
    pub max_health: f32,
    /// Food level, 0..20.
    pub food: i32,
    /// Block and sky light at the player's block, 0..15 each.
    pub light: [u8; 2],
    /// `hurtTime`.
    pub hurt_time: i32,
    /// `!isAlive()`.
    pub dead: bool,
}

/// `Entity.getPercentFrozen`'s full freeze, in ticks.
pub const FREEZE_TICKS: i32 = 140;

/// Fills `input`'s action, hurt, dying, health, hunger and light values from `s`, as CPM's
/// `PlayerProfile.updateFromPlayer` / `updateFromState` and `PlayerAnimUpdater` fill
/// `AnimationState` from the game.
pub fn action_input(s: &ActionState, input: &mut PoseInput) {
    input.attack = s.swing;
    let used = s.using.and_then(|u| {
        let pull = |max: u32| u.ticks.min(max) as f32 / max as f32;
        let progress = match u.pull {
            Pull::None => 0.0,
            Pull::Bow => pull(20),
            Pull::Crossbow(max) => pull(max.max(1)),
        };
        u.kind.map(|k| (u.hand, k, progress))
    });
    input.using = used;
    // `getArmPose`, each hand on its own: a charged crossbow held while not swinging.
    input.crossbow_charged = s.charged.map(|c| c && !s.swinging);
    input.holding = s.holding;
    input.on_ladder = s.on_climbable;
    input.climbing = s.dy.abs() > 0.05;
    input.crawling = s.visually_swimming && !s.in_water;
    input.in_water = s.in_water;
    input.trident_spin = s.spin_attack;
    input.freezing = s.ticks_frozen.clamp(0, FREEZE_TICKS) as f32 / FREEZE_TICKS as f32;
    input.health = (s.max_health > 0.0).then(|| s.health / s.max_health);
    input.hunger = Some(s.food as f32 / 20.0);
    input.light = Some(s.light[0].max(s.light[1]) as f32 / 15.0);
    input.hurt = s.hurt_time > 0;
    input.dying = s.dead;
}

/// The models folder, `<game dir>/player_models`, from the mod's data folder
/// (`<game dir>/horizon-mods/.horizon-mod-data/cpm`); made when missing (a failure is logged).
pub fn models_dir(data_dir: &Path) -> PathBuf {
    let game = data_dir.ancestors().nth(3).unwrap_or(data_dir);
    let dir = game.join("player_models");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("cpm: cannot make {}: {e}", dir.display());
    }
    dir
}

/// The `*.cpmmodel` files in `dir`, sorted by path; none when it cannot be read.
pub fn list_models(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut models: Vec<PathBuf> = read
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("cpmmodel")))
        .collect();
    models.sort();
    models
}

/// The setting the selected model's file name is kept under.
pub const MODEL_SETTING: &str = "model";

/// The stored `model` setting (a file name in `dir`) as a selection; `None` when unset, not a
/// plain file name, or the file is gone (the vanilla player). [`SKIN_SETTING`] is the skin's model
/// ([`skin_selection`]), drawn while the skin holds one.
pub fn selection_from_setting(dir: &Path, stored: Option<String>) -> Option<PathBuf> {
    let name = stored?;
    if name == SKIN_SETTING {
        return Some(skin_selection());
    }
    let mut parts = Path::new(&name).components();
    let (Some(std::path::Component::Normal(_)), None) = (parts.next(), parts.next()) else {
        return None;
    };
    let path = dir.join(&name);
    path.is_file().then_some(path)
}

/// The value the `model` setting keeps for `selected`: its file name ([`SKIN_SETTING`] for the
/// skin's model), "" for none.
pub fn model_setting(selected: Option<&Path>) -> String {
    match selected {
        Some(p) if is_skin(p) => SKIN_SETTING.to_owned(),
        _ => file_name(selected),
    }
}

/// The id of the mod's one key (`NativeMod.keys`): it opens the gesture menu.
pub const MENU_KEY: &str = "gesture_menu";
/// The menu key's default binding.
pub const DEFAULT_MENU_KEY: &str = "key.keyboard.g";

/// The mod's keys for `NativeMod.keys`: just the gesture menu.
pub fn keys() -> Vec<[String; 3]> {
    vec![[MENU_KEY.to_owned(), "Gesture menu".to_owned(), DEFAULT_MENU_KEY.to_owned()]]
}

/// Asks for the gesture menu (the menu key or the settings page's `Gestures…`). The frame hook
/// opens it ([`take_menu_request`]).
pub fn request_menu() {
    STATE.with_borrow_mut(|s| s.menu_requested = true);
}

/// Whether the menu was asked for since the last call; clears the request.
pub fn take_menu_request() -> bool {
    STATE.with_borrow_mut(|s| std::mem::take(&mut s.menu_requested))
}

/// Whether the menu is asked for, without clearing the request: the frame hook keeps it until
/// the screen can open (the settings page's `Gestures…` waits out the menus).
pub fn menu_requested() -> bool {
    STATE.with_borrow(|s| s.menu_requested)
}

/// Whether the menu key opens the menu: only while playing, with no screen up (CPM's own
/// included).
pub fn menu_key_allowed(playing: bool, other_screen_open: bool) -> bool {
    playing && !other_screen_open
}

// ---- The gesture menu: a mod screen (gesture_screen.rs draws it and feeds it input) ----
// The layout mirrors CPM's `GestureGui`: a top row (Reset Gesture, Reset Pose), then the model's
// visible buttons in 80x30 cells, 90 apart across (at most 4 columns) and 40 down. Where CPM
// scrolls, this pages.

/// The menu's title.
pub const MENU_TITLE: &str = "Gestures";
/// Shown when the model has no visible buttons (or no model is loaded).
pub const NO_GESTURES: &str = "This model has no gestures";
pub const RESET_GESTURE: &str = "Reset Gesture";
pub const RESET_POSE: &str = "Reset Pose";

/// A cell (`GestureGui`: 80x30, a 90x40 grid, 4 columns).
pub const CELL_W: f32 = 80.0;
pub const CELL_H: f32 = 30.0;
const STEP_X: f32 = 90.0;
const STEP_Y: f32 = 40.0;
const MAX_COLUMNS: usize = 4;
/// The top row's buttons (`GestureGui`: 100x20, 110 apart).
const TOP_W: f32 = 100.0;
const TOP_H: f32 = 20.0;
const TOP_STEP: f32 = 110.0;
/// Space kept free round the menu.
const MENU_MARGIN: f32 = 8.0;
/// The screen height `GestureGui` keeps for everything but the grid (`h < height - 150`).
const GRID_RESERVED: f32 = 150.0;
/// The title's row (text and the gap under it).
const TITLE_ROW: f32 = 14.0;
/// The gap between rows of controls (the top row, the grid, the page bar).
const ROW_GAP: f32 = 10.0;
/// The `<` and `>` page buttons' side.
const PAGE_BUTTON: f32 = 20.0;
/// A dropdown's label row over its box (`GestureDropdown`: 10).
pub const DROPDOWN_LABEL_H: f32 = 10.0;
/// A row of an open dropdown's list.
pub const DROPDOWN_ROW_H: f32 = 12.0;
/// A slider's marker width.
pub const SLIDER_MARKER_W: f32 = 8.0;

/// One of the model's buttons on the menu: its index into `Engine::buttons` and its cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Widget {
    pub button: usize,
    /// `[x, y, w, h]` in GUI pixels.
    pub rect: [f32; 4],
}

/// Where the menu's parts are, in GUI pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuLayout {
    /// The visible buttons, page by page (none at all: no pages).
    pub pages: Vec<Vec<Widget>>,
    /// The title's top.
    pub title_y: f32,
    pub reset_gesture: [f32; 4],
    pub reset_pose: [f32; 4],
    /// The page buttons, with more than one page.
    pub prev: Option<[f32; 4]>,
    pub next: Option<[f32; 4]>,
    /// The top of the grid (where [`NO_GESTURES`] goes when there are no pages).
    pub grid_y: f32,
}

/// What a click on the menu is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    ResetGesture,
    ResetPose,
    Prev,
    Next,
    /// The button at this index of `Engine::buttons`.
    Button(usize),
}

/// The menu for `buttons` (hidden ones left out) on a `gui_w` x `gui_h` screen: centred, as
/// many columns as fit (at most 4), and as many rows per page as fit with the page bar.
pub fn layout_menu(buttons: &[cpm_core::anim::Button], gui_w: f32, gui_h: f32) -> MenuLayout {
    let visible: Vec<usize> = (0..buttons.len()).filter(|&i| !buttons[i].hidden).collect();
    let gap_x = STEP_X - CELL_W;
    let cols = (((gui_w - 2.0 * MENU_MARGIN + gap_x) / STEP_X).floor().max(1.0) as usize).min(MAX_COLUMNS);
    let grid_w = cols as f32 * STEP_X - gap_x;
    let block_w = grid_w.max(TOP_STEP + TOP_W);
    let x0 = ((gui_w - block_w) / 2.0).max(0.0).floor();
    let grid_x = x0 + ((block_w - grid_w) / 2.0).floor();
    let header = TITLE_ROW + TOP_H + ROW_GAP;
    let page_bar = ROW_GAP + PAGE_BUTTON;
    // CPM's rule: a grid of `r` rows (r * 40 tall) shows whole when shorter than the height less
    // GRID_RESERVED; past that CPM scrolls, this pages.
    let rows_fit = (((gui_h - GRID_RESERVED) / STEP_Y).ceil() as i64 - 1).max(1) as usize;
    let n = visible.len();
    let (rows, paged) = if n.div_ceil(cols) <= rows_fit { (n.div_ceil(cols), false) } else { (rows_fit, true) };
    let grid_h = if rows == 0 { 0.0 } else { rows as f32 * STEP_Y - ROW_GAP };
    let block_h = header + grid_h + if paged { page_bar } else { 0.0 };
    let top = ((gui_h - block_h) / 2.0).max(MENU_MARGIN).floor();
    let top_y = top + TITLE_ROW;
    let grid_y = top_y + TOP_H + ROW_GAP;
    let per_page = (rows * cols).max(1);
    let pages = visible
        .chunks(per_page)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .map(|(k, &button)| {
                    let (col, row) = ((k % cols) as f32, (k / cols) as f32);
                    Widget { button, rect: [grid_x + col * STEP_X, grid_y + row * STEP_Y, CELL_W, CELL_H] }
                })
                .collect()
        })
        .collect();
    let bar_y = grid_y + grid_h + ROW_GAP;
    MenuLayout {
        pages,
        title_y: top,
        reset_gesture: [x0, top_y, TOP_W, TOP_H],
        reset_pose: [x0 + TOP_STEP, top_y, TOP_W, TOP_H],
        prev: paged.then_some([grid_x, bar_y, PAGE_BUTTON, PAGE_BUTTON]),
        next: paged.then_some([grid_x + grid_w - PAGE_BUTTON, bar_y, PAGE_BUTTON, PAGE_BUTTON]),
        grid_y,
    }
}

/// What `(x, y)` is on, with page `page` shown.
pub fn hit(layout: &MenuLayout, page: usize, x: f32, y: f32) -> Option<Hit> {
    let at = [x, y];
    if inside(layout.reset_gesture, at) {
        return Some(Hit::ResetGesture);
    }
    if inside(layout.reset_pose, at) {
        return Some(Hit::ResetPose);
    }
    if layout.prev.is_some_and(|r| inside(r, at)) {
        return Some(Hit::Prev);
    }
    if layout.next.is_some_and(|r| inside(r, at)) {
        return Some(Hit::Next);
    }
    let widgets = layout.pages.get(page)?;
    widgets.iter().find(|w| inside(w.rect, at)).map(|w| Hit::Button(w.button))
}

/// The slider value at pointer `x` on `widget`'s track (0..=`max`, clamped at the ends).
pub fn slider_value(widget: &Widget, x: f32, max: u8) -> u8 {
    let [rx, _, w, _] = widget.rect;
    let span = (w - SLIDER_MARKER_W).max(1.0);
    let t = ((x - rx - SLIDER_MARKER_W / 2.0) / span).clamp(0.0, 1.0);
    (t * max as f32 + 0.5).floor().min(max as f32) as u8
}

/// The left edge of the slider's marker for `value` (of `max`).
pub fn slider_marker_x(widget: &Widget, value: u8, max: u8) -> f32 {
    let [rx, _, w, _] = widget.rect;
    let t = if max == 0 { 0.0 } else { value.min(max) as f32 / max as f32 };
    rx + t * (w - SLIDER_MARKER_W)
}

/// The rows of a dropdown's open list for its cell `cell` (`n` options): under its box, or over
/// it when they would run off a `gui_h` tall screen.
pub fn dropdown_list(cell: [f32; 4], n: usize, gui_h: f32) -> Vec<[f32; 4]> {
    let [x, y, w, h] = cell;
    let total = n as f32 * DROPDOWN_ROW_H;
    let below = y + h;
    let top = if below + total <= gui_h { below } else { (y + DROPDOWN_LABEL_H - total).max(0.0) };
    (0..n).map(|i| [x, top + i as f32 * DROPDOWN_ROW_H, w, DROPDOWN_ROW_H]).collect()
}

/// The option of an open list (`dropdown_list`) under `at`.
pub fn option_at(list: &[[f32; 4]], at: [f32; 2]) -> Option<usize> {
    list.iter().position(|r| inside(*r, at))
}

/// How a dropdown option is shown: option 0 (`""`) is `None`.
pub fn option_label(option: &str) -> &str {
    if option.is_empty() { "None" } else { option }
}

/// The menu on screen: its page, the dropdown whose list is open and the slider being dragged
/// (indices into `Engine::buttons`), and how its preview is turned and whether a drag turns it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Menu {
    pub page: usize,
    pub open_dropdown: Option<usize>,
    pub dragging: Option<usize>,
    pub preview: PreviewView,
    /// A left-button drag that began on the preview turns it.
    pub rotating: bool,
}

/// CPM's own mod screens: one is open at a time (Horizon gives a mod one screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModScreen {
    Gist,
    Menu,
}

/// Which of CPM's screens is shown, if one is.
pub fn shown_screen() -> Option<ModScreen> {
    STATE.with_borrow(|s| {
        if s.gist.is_some() {
            Some(ModScreen::Gist)
        } else if s.menu.is_some() {
            Some(ModScreen::Menu)
        } else {
            None
        }
    })
}

/// Shows the menu at its first page, the preview turned as it starts (the mod screen was just
/// opened for it).
pub fn show_menu() {
    STATE.with_borrow_mut(|s| s.menu = Some(Menu::default()));
}

/// Drops the menu (its screen closed).
pub fn hide_menu() {
    STATE.with_borrow_mut(|s| {
        s.menu = None;
        drop_picker(s);
    });
}

/// Runs `f` on the shown menu; `None` when it is not shown.
pub fn with_menu<R>(f: impl FnOnce(&mut Menu) -> R) -> Option<R> {
    STATE.with_borrow_mut(|s| s.menu.as_mut().map(f))
}

// ---- The gesture menu's 3D preview: the model posed live on the menu's right ----
// gesture_screen.rs draws `preview_quads` with `HudBatch::quad_3d` and `depth_since`, as Horizon
// draws its inventory player (game/perspective.rs `draw_player_preview`): an orthographic view,
// faces shaded by their normal (`entity_in_ui_shade`), back faces left out, far to near.

/// The preview's turn when the menu opens, degrees.
pub const PREVIEW_YAW: f32 = 30.0;
/// The preview's tilt either way at most, degrees.
pub const PREVIEW_PITCH_MAX: f32 = 60.0;
pub const PREVIEW_ZOOM_MIN: f32 = 0.5;
pub const PREVIEW_ZOOM_MAX: f32 = 3.0;
/// Degrees the preview turns per GUI pixel dragged.
const PREVIEW_DEG_PER_PX: f32 = 1.0;
/// The zoom factor per wheel notch.
const PREVIEW_ZOOM_STEP: f32 = 1.1;
/// How much of the rect's smaller side the model's bounds take at zoom 1.
const PREVIEW_FILL: f32 = 0.9;
/// The share of the screen's width the preview takes, on the right.
const PREVIEW_SHARE: f32 = 0.4;
/// The world draw's input is used for this long after it was recorded; past that (the world no
/// longer draws the player: first person) the preview stands.
pub const PREVIEW_INPUT_STALE_MS: u64 = 1000;

/// How the preview is turned (degrees) and zoomed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewView {
    /// Turn about the model's up axis; 0 faces the viewer, positive turns its front to the right.
    pub yaw: f32,
    /// Tilt; positive shows the top. Within ±[`PREVIEW_PITCH_MAX`].
    pub pitch: f32,
    /// Within [`PREVIEW_ZOOM_MIN`]..=[`PREVIEW_ZOOM_MAX`]; at 1 the model fits its rect.
    pub zoom: f32,
}

impl Default for PreviewView {
    fn default() -> Self {
        PreviewView { yaw: PREVIEW_YAW, pitch: 0.0, zoom: 1.0 }
    }
}

impl PreviewView {
    /// The pointer dragged `dx`, `dy` GUI pixels: right turns the front right, down tilts the top
    /// toward the viewer.
    pub fn drag(&mut self, dx: f32, dy: f32) {
        self.yaw = (self.yaw + dx * PREVIEW_DEG_PER_PX).rem_euclid(360.0);
        self.pitch = (self.pitch + dy * PREVIEW_DEG_PER_PX).clamp(-PREVIEW_PITCH_MAX, PREVIEW_PITCH_MAX);
    }

    /// The wheel turned `steps` notches: up zooms in.
    pub fn scroll(&mut self, steps: f32) {
        let zoom = self.zoom * PREVIEW_ZOOM_STEP.powf(steps);
        self.zoom = if zoom.is_nan() { 1.0 } else { zoom.clamp(PREVIEW_ZOOM_MIN, PREVIEW_ZOOM_MAX) };
    }
}

/// The menu split on a `gui_w` x `gui_h` screen: the controls' width (from the left; lay the
/// controls out in it) and the preview's rect `[x, y, w, h]` on the right.
pub fn menu_split(gui_w: f32, gui_h: f32) -> (f32, [f32; 4]) {
    let controls = (gui_w * (1.0 - PREVIEW_SHARE)).floor();
    let preview = [controls, MENU_MARGIN, (gui_w - controls - MENU_MARGIN).max(0.0), (gui_h - 2.0 * MENU_MARGIN).max(0.0)];
    (controls, preview)
}

/// Which face of which box a [`PreviewFace`] is: `boxes[box_index].faces[face]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UvSource {
    pub box_index: usize,
    pub face: usize,
}

/// One face of the preview.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewFace {
    /// GUI pixels (x right, y down) and depth (+z toward the viewer), in the face's own corner
    /// order (TL, BL, BR, TR).
    pub corners: [[f32; 3]; 4],
    pub uv_src: UvSource,
    /// The box's 0xRRGGBB tint.
    pub tint: u32,
    /// The light on it, 0.4..=1 (1 for a glowing box).
    pub shade: f32,
}

/// `entity_in_ui_shade` (Horizon, game/perspective.rs): the GUI's two lights on outward normal `n`
/// (GUI space: y down, +z toward the viewer).
fn ui_shade(n: [f32; 3]) -> f32 {
    let lit = |l: [f32; 3]| {
        let len = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        ((l[0] * n[0] + l[1] * n[1] + l[2] * n[2]) / len).max(0.0)
    };
    ((lit([0.2, -1.0, 1.0]) + lit([-0.2, -1.0, 0.0])) * 0.6 + 0.4).min(1.0)
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// Where the preview centres a model and how big it is: its bounds' centre (blocks, draw frame)
/// and its half width and half height (blocks): the half width is its farthest reach round the
/// vertical axis, so turning it (the preview's drag) never takes it past the rect's sides.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewFit {
    pub centre: [f32; 3],
    pub half: [f32; 2],
}

/// Every face of `boxes` in the draw frame (blocks, Y up, +Z the model's front): (box, face,
/// corners).
fn world_faces(boxes: &[PosedBox]) -> Vec<(usize, usize, [[f32; 3]; 4])> {
    boxes
        .iter()
        .enumerate()
        .flat_map(|(bi, b)| {
            b.faces.iter().enumerate().filter_map(move |(fi, f)| {
                let f = f.as_ref()?;
                Some((bi, fi, f.corners.map(|c| b.transform.transform_point3(c.into()).to_array())))
            })
        })
        .collect()
}

/// `p` (relative to the fit's centre) as `view` sees it: the turn, then the tilt.
fn view_turn(view: &PreviewView, [x, y, z]: [f32; 3]) -> [f32; 3] {
    let (sy, cy) = view.yaw.to_radians().sin_cos();
    let (sp, cp) = view.pitch.to_radians().sin_cos();
    let (x, z) = (x * cy + z * sy, -x * sy + z * cy);
    let (y, z) = (y * cp - z * sp, y * sp + z * cp);
    [x, y, z]
}

/// The fit of `boxes`' bounds; `None` when they have no face.
pub fn preview_fit(boxes: &[PosedBox]) -> Option<PreviewFit> {
    let mut lo = [f32::INFINITY; 3];
    let mut hi = [f32::NEG_INFINITY; 3];
    let world = world_faces(boxes);
    for c in world.iter().flat_map(|(_, _, c)| c).filter(|c| c.iter().all(|v| v.is_finite())) {
        for k in 0..3 {
            lo[k] = lo[k].min(c[k]);
            hi[k] = hi[k].max(c[k]);
        }
    }
    if !lo[0].is_finite() {
        return None;
    }
    let centre = [0, 1, 2].map(|k| (lo[k] + hi[k]) / 2.0);
    let mut half = [1e-3f32; 2];
    for c in world.iter().flat_map(|(_, _, c)| c).filter(|c| c.iter().all(|v| v.is_finite())) {
        let [x, y, z] = sub(*c, centre);
        half = [half[0].max((x * x + z * z).sqrt()), half[1].max(y.abs())];
    }
    Some(PreviewFit { centre, half })
}

/// The fit of `model` at rest: posed with no animation, standing, wearing nothing (the engine is
/// not sampled). The live preview keeps it, so a gesture moves the model in a still frame instead
/// of rescaling it; a pose past it is cut at the rect.
pub fn rest_fit(model: &LoadedModel) -> Option<PreviewFit> {
    let standing = PoseInput { on_ground: true, ..PoseInput::default() };
    preview_fit(&cpm_core::mesh::pose_model(&model.model, &cpm_core::Frame::default(), &standing))
}

/// `boxes` seen as `view` says, fitted to `rect` by their own bounds ([`preview_fit`]); see
/// [`preview_faces_in`]. Only the tests ask (the game keeps the rest fit: [`preview_quads`]).
#[allow(dead_code)]
pub fn preview_faces(boxes: &[PosedBox], view: &PreviewView, rect: [f32; 4]) -> Vec<PreviewFace> {
    match preview_fit(boxes) {
        Some(fit) => preview_faces_in(boxes, &fit, view, rect),
        None => Vec::new(),
    }
}

/// `boxes` seen as `view` says, `fit` fitted to `rect` (GUI pixels): at zoom 1 and the default
/// whichever way it is turned round, the model stays within [`PREVIEW_FILL`] of the rect's width
/// and height; tilted or zoomed past that it is cut at the rect. Faces looking away
/// are left out, except an elytra's (both sides show, as the game's `WingsLayer` draws them).
pub fn preview_faces_in(boxes: &[PosedBox], fit: &PreviewFit, view: &PreviewView, rect: [f32; 4]) -> Vec<PreviewFace> {
    let world = world_faces(boxes);
    let centre = fit.centre;
    let [rx, ry, rw, rh] = rect;
    let scale = (rw / 2.0 / fit.half[0]).min(rh / 2.0 / fit.half[1]) * PREVIEW_FILL * view.zoom;
    let (gx, gy) = (rx + rw / 2.0, ry + rh / 2.0);
    let turn = |p: [f32; 3]| view_turn(view, sub(p, centre));
    let mut out = Vec::new();
    for (bi, fi, corners) in world {
        let b = &boxes[bi];
        let r = corners.map(turn);
        // `(bl - tl) × (br - tl)` points out of the box in its own space; a mirroring transform
        // turns it in.
        let mut n = cross(sub(r[1], r[0]), sub(r[2], r[0]));
        if b.transform.determinant() < 0.0 {
            n = n.map(|v| -v);
        }
        if n[2] <= 0.0 && b.texture != BoxTexture::Elytra {
            continue;
        }
        // In GUI space (y down).
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        let n = if len > 0.0 { [n[0] / len, -n[1] / len, n[2] / len] } else { [0.0; 3] };
        out.push(PreviewFace {
            corners: r.map(|[x, y, z]| [gx + x * scale, gy - y * scale, z * scale]),
            uv_src: UvSource { box_index: bi, face: fi },
            tint: b.tint,
            shade: if b.glow { 1.0 } else { ui_shade(n) },
        });
    }
    out
}

/// One quad of the preview for `HudBatch::quad_3d`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreviewQuad {
    /// GUI pixels and depth (+z toward the viewer), wound as the game's GUI previews are.
    pub corners: [[f32; 3]; 4],
    /// Per corner, texels (16 a side) of its sprite: the atlas UV is the sprite's rect at
    /// `uv / 16`.
    pub uv: [[f32; 2]; 4],
    /// Index into [`PreviewQuads::sprites`].
    pub sprite: usize,
    /// The shaded tint as the GUI batch takes a colour (0xAABBGGRR, opaque).
    pub color: u32,
}

/// The preview's quads, far to near, and the sprites they sample (the first is [`SPRITE`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewQuads {
    pub sprites: Vec<String>,
    pub quads: Vec<PreviewQuad>,
}

/// 0xRRGGBB `tint` times `shade`, opaque, as the GUI batch's colour (`hud::rgba`).
fn gui_color(tint: u32, shade: f32) -> u32 {
    let ch = |s: u32| ((((tint >> s) & 255) as f32 * shade) as u32).min(255);
    0xFF00_0000 | ch(0) << 16 | ch(8) << 8 | ch(16)
}

/// `boxes` (posed from `model`, wearing `worn`) as the preview draws them in `rect`: each face of
/// [`preview_faces_in`] (fitted by `fit`) in each of its box's passes, sampled as the world draw ([`box_quads`]) samples
/// it.
pub fn preview_box_quads(
    model: &LoadedModel,
    boxes: &[PosedBox],
    worn: &Worn,
    fit: &PreviewFit,
    view: &PreviewView,
    rect: [f32; 4],
) -> PreviewQuads {
    preview_box_quads_at(model, boxes, worn, fit, view, rect, [0, 0])
}

/// [`preview_box_quads`] for a model whose image is at `base` in [`SPRITE`].
fn preview_box_quads_at(
    model: &LoadedModel,
    boxes: &[PosedBox],
    worn: &Worn,
    fit: &PreviewFit,
    view: &PreviewView,
    rect: [f32; 4],
    base: [u32; 2],
) -> PreviewQuads {
    let mut sprites = Quads::new();
    let passes: Vec<Vec<Pass>> = boxes
        .iter()
        .map(|b| {
            let mut p = Vec::new();
            box_passes(model, b, worn, base, &mut sprites, &mut p);
            p
        })
        .collect();
    let mut quads: Vec<(f32, PreviewQuad)> = Vec::new();
    for f in preview_faces_in(boxes, fit, view, rect) {
        let b = &boxes[f.uv_src.box_index];
        let Some(face) = &b.faces[f.uv_src.face] else { continue };
        let depth = f.corners.iter().map(|c| c[2]).sum::<f32>() / 4.0;
        for pass in &passes[f.uv_src.box_index] {
            let ([u0, v0, u1, v1], sprite, tint) = pass.map(model, b, face, base);
            let mut corners = f.corners;
            let mut uv = [[u0, v0], [u0, v1], [u1, v1], [u1, v0]];
            // The game's previews turn front faces negatively in z (game/perspective.rs
            // `player_preview_quads`): a mirrored box or a back face is turned round.
            let (a, c) = (sub(corners[1], corners[0]), sub(corners[2], corners[0]));
            if a[0] * c[1] - a[1] * c[0] > 0.0 {
                corners.reverse();
                uv.reverse();
            }
            // An emissive overlay is full bright (the GUI draws no glint).
            let shade = if pass.emissive { 1.0 } else { f.shade };
            quads.push((depth, PreviewQuad { corners, uv, sprite, color: gui_color(tint, shade) }));
        }
    }
    // Far to near (stable: a box's passes stay in order).
    quads.sort_by(|a, b| a.0.total_cmp(&b.0));
    PreviewQuads { sprites: sprites.sprites, quads: quads.into_iter().map(|(_, q)| q).collect() }
}

/// The loaded model posed for `input` wearing `worn` (as [`quads`] poses it), as the preview in
/// `rect` draws it, in the model's [`rest_fit`] (worked out once per load); `None` as for
/// [`quads`], or for a model with no faces.
pub fn preview_quads(input: &PoseInput, worn: &Worn, view: &PreviewView, rect: [f32; 4]) -> Option<PreviewQuads> {
    STATE.with_borrow_mut(|s| {
        if s.uploaded.is_none() {
            return None;
        }
        let m = s.model.as_mut()?;
        let input = PoseInput { equipment: worn.equipment(elytra_sheet(m).is_some()), ..*input };
        let boxes = m.pose(&input);
        // The first frame the preview draws sets the fit, animated as it is seen: a model's
        // unanimated base pose can be far wider (rainwing's wings spread to 5 blocks each side
        // until its standing animation folds them).
        let fit = match s.preview_fit {
            Some(fit) => fit,
            None => {
                let fit = preview_fit(&boxes)?;
                *s.preview_fit.insert(fit)
            }
        };
        Some(preview_box_quads(m, &boxes, worn, &fit, view, rect))
    })
}

/// The fit [`preview_quads`] keeps for the loaded model, once it drew. Only the tests ask.
#[allow(dead_code)]
pub fn kept_preview_fit() -> Option<PreviewFit> {
    STATE.with_borrow(|s| s.preview_fit)
}

/// Keeps the input the world draw posed the model with, for the preview.
pub fn record_input(input: &PoseInput) {
    STATE.with_borrow_mut(|s| s.last_input = Some(*input));
}

/// The input the preview poses the model with at `now_ms`: the world draw's last
/// ([`record_input`]) at the new time, so the engine sees no other movement than the world's (its
/// jump and pose changes are not disturbed); standing on the ground when there is none, or none
/// within [`PREVIEW_INPUT_STALE_MS`] (its cape then hanging still). The preview draws no items, so
/// its hands hold nothing (no holding, item-use or charged-crossbow poses).
pub fn preview_input(now_ms: u64) -> PoseInput {
    let last = STATE.with_borrow(|s| s.last_input);
    match last.filter(|i| now_ms.saturating_sub(i.time_ms) <= PREVIEW_INPUT_STALE_MS) {
        Some(i) => PoseInput {
            time_ms: now_ms.max(i.time_ms),
            holding: [false; 2],
            crossbow_charged: [false; 2],
            using: None,
            ..i
        },
        // No world draw to copy (first person): the cape hangs still.
        None => PoseInput {
            time_ms: now_ms,
            on_ground: true,
            cape: Some(cpm_core::CapeState::default()),
            ..PoseInput::default()
        },
    }
}

// ---- The Models picker: a page of the gesture menu's screen (gesture_screen.rs draws it) ----
// The `player_models` files as icon tiles, page by page; a click previews that model in 3D on the
// right, `Use this model` selects it. The files are read on threads of their own (their icons when
// the picker opens, the clicked one's model); the outcomes cross back through a process-wide
// `Mutex` ([`poll_picker`]), not `State`. Their images join the selected model's in [`SPRITE`]
// ([`slot_layout`], [`texture_upload`]).

/// The gestures page's button that opens the picker.
pub const MODELS: &str = "Models";
/// The picker's button that selects the previewed model.
pub const USE_MODEL: &str = "Use this model";
/// The picker's button back to the gestures.
pub const BACK: &str = "Back";
/// The picker with no files.
pub const NO_MODELS: &str = "No .cpmmodel files in player_models";
/// The previewed model's image has no room in [`SPRITE`].
pub const TOO_LARGE: &str = "Too large to preview";
/// The file does not read or decode (its tile and its preview say so).
pub const CANT_READ: &str = "Can't read this model";
/// A link model whose gist text is not cached yet: selecting it downloads it.
pub const NOT_CACHED: &str = "Select it once to download it";
/// An icon's side in [`SPRITE`] and on screen, pixels.
pub const ICON_PX: u32 = 32;
/// A tile on the picker: the icon over the name.
const TILE_W: f32 = 48.0;
const TILE_H: f32 = 46.0;
const TILE_GAP: f32 = 4.0;
/// The `Models` button's width (at the gestures page's bottom left).
const MODELS_W: f32 = 60.0;
const BACK_W: f32 = 50.0;
const USE_W: f32 = 100.0;

/// Where the parts of the picker's image go in [`SPRITE`], beside the selected model's (which
/// keeps the top left, `active` pixels: [`composite`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotLayout {
    /// The previewed model's image ([`composite`]); `None`: no room ([`TOO_LARGE`]).
    pub preview_at: Option<[u32; 2]>,
    /// Each icon's [`ICON_PX`] square, in order; `None`: no room (a name tile).
    pub icon_at: Vec<Option<[u32; 2]>>,
}

/// The layout for the selected model's image (`active` pixels, `[0, 0]` for none), a previewed
/// model's image of `preview` pixels, and `icons` icons. The preview goes under the active image
/// at the left edge when it fits the sprite. The icons fill a grid of [`ICON_PX`] squares to the
/// right of both, then the whole width below them, row by row; those past the room left are
/// `None`.
pub fn slot_layout(active: [u32; 2], preview: Option<[u32; 2]>, icons: usize) -> SlotLayout {
    let [aw, ah] = active;
    let fits = |[pw, ph]: [u32; 2]| pw <= SPRITE_PX && ah.checked_add(ph).is_some_and(|b| b <= SPRITE_PX);
    let placed = preview.filter(|p| fits(*p));
    let preview_at = placed.map(|_| [0, ah]);
    let (right, bottom) = match placed {
        Some([pw, ph]) => (aw.max(pw), ah + ph),
        None => (aw, ah),
    };
    let mut spots = Vec::with_capacity(icons);
    // Right of the images, down to their bottom.
    let mut y = 0;
    while y + ICON_PX <= bottom.min(SPRITE_PX) && spots.len() < icons {
        let mut x = right;
        while x + ICON_PX <= SPRITE_PX && spots.len() < icons {
            spots.push([x, y]);
            x += ICON_PX;
        }
        y += ICON_PX;
    }
    // Under them, the whole width.
    let mut y = bottom;
    while y + ICON_PX <= SPRITE_PX && spots.len() < icons {
        let mut x = 0;
        while x + ICON_PX <= SPRITE_PX && spots.len() < icons {
            spots.push([x, y]);
            x += ICON_PX;
        }
        y += ICON_PX;
    }
    SlotLayout { preview_at, icon_at: (0..icons).map(|i| spots.get(i).copied()).collect() }
}

/// The picker's tiles, page by page: tile `k` of page `p` is file `p * per_page + k`.
#[derive(Debug, Clone, PartialEq)]
pub struct PickerGrid {
    pub per_page: usize,
    /// Each tile's `[x, y, w, h]` (GUI pixels).
    pub pages: Vec<Vec<[f32; 4]>>,
}

/// `n` tiles in `rect`: as many columns and rows as fit (one at least), left to right, top down.
pub fn picker_grid(n: usize, rect: [f32; 4]) -> PickerGrid {
    let [x, y, w, h] = rect;
    let count = |room: f32, tile: f32| (((room + TILE_GAP) / (tile + TILE_GAP)).floor().max(1.0)) as usize;
    let (cols, rows) = (count(w, TILE_W), count(h, TILE_H));
    let per_page = cols * rows;
    let pages = (0..n)
        .collect::<Vec<_>>()
        .chunks(per_page)
        .map(|chunk| {
            (0..chunk.len())
                .map(|k| {
                    let (col, row) = ((k % cols) as f32, (k / cols) as f32);
                    [x + col * (TILE_W + TILE_GAP), y + row * (TILE_H + TILE_GAP), TILE_W, TILE_H]
                })
                .collect()
        })
        .collect();
    PickerGrid { per_page, pages }
}

impl PickerGrid {
    /// The file whose tile is under `at` on page `page`.
    pub fn hit(&self, page: usize, at: [f32; 2]) -> Option<usize> {
        let k = self.pages.get(page)?.iter().position(|r| inside(*r, at))?;
        Some(page * self.per_page + k)
    }
}

/// The `Models` button: the gestures page's bottom left (of a `gui_h` high screen).
pub fn models_button(gui_h: f32) -> [f32; 4] {
    [MENU_MARGIN, (gui_h - MENU_MARGIN - TOP_H).max(0.0), MODELS_W, TOP_H]
}

/// Where the picker page's parts are, in GUI pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickerLayout {
    /// The title's top ([`MODELS`]).
    pub title_y: f32,
    /// The tiles' room ([`picker_grid`]).
    pub grid: [f32; 4],
    pub back: [f32; 4],
    pub use_model: [f32; 4],
    /// The page buttons (shown with more than one page).
    pub prev: [f32; 4],
    pub next: [f32; 4],
}

/// The picker page in the controls' `controls_w` (from the left) of a `gui_h` high screen: the
/// title, the tiles, and a bottom row of `Back`, `Use this model` and the page buttons.
pub fn picker_layout(controls_w: f32, gui_h: f32) -> PickerLayout {
    let title_y = MENU_MARGIN;
    let grid_y = title_y + TITLE_ROW;
    let bar_y = (gui_h - MENU_MARGIN - TOP_H).max(grid_y);
    let right = controls_w - MENU_MARGIN;
    PickerLayout {
        title_y,
        grid: [MENU_MARGIN, grid_y, (right - MENU_MARGIN).max(0.0), (bar_y - ROW_GAP - grid_y).max(0.0)],
        back: [MENU_MARGIN, bar_y, BACK_W, TOP_H],
        use_model: [MENU_MARGIN + BACK_W + TILE_GAP, bar_y, USE_W, TOP_H],
        prev: [right - 2.0 * PAGE_BUTTON - TILE_GAP, bar_y, PAGE_BUTTON, PAGE_BUTTON],
        next: [right - PAGE_BUTTON, bar_y, PAGE_BUTTON, PAGE_BUTTON],
    }
}

/// The settings page's note for a model made by an older converter (`stem`: its file stem).
pub fn old_conversion_note(stem: &str) -> String {
    format!("Old conversion: convert {stem}.cpmproject again")
}

/// A model loaded for the picker's preview, with its own engine (default parameters).
pub struct PreviewModel {
    pub model: LoadedModel,
    /// Its [`rest_fit`]; `None` for a model with no faces.
    pub fit: Option<PreviewFit>,
    /// The fit of the first frame the preview drew (animated); what the preview uses.
    pub live_fit: Option<PreviewFit>,
    /// Made by an older converter ([`old_conversion_note`]).
    pub old_conversion: bool,
}

/// Loads `path` for the preview, as [`ensure_loaded`] loads a selection, but a link model only
/// from the gist cache in `data_dir` ([`NOT_CACHED`] without it: nothing is downloaded).
/// Anything else that fails is [`CANT_READ`] (the reason logged).
pub fn load_preview(path: &Path, data_dir: Option<&Path>) -> Result<PreviewModel, String> {
    let name = file_name(Some(path));
    let bytes = std::fs::read(path).map_err(|e| {
        log::warn!("cpm: preview {name}: {e}");
        CANT_READ.to_owned()
    })?;
    preview_bytes(&name, &bytes, data_dir, None)
}

/// [`load_preview`] of the model file `bytes` (`name` in messages); a model without a texture of
/// its own gets `skin` (the skin's model).
fn preview_bytes(
    name: &str,
    bytes: &[u8],
    data_dir: Option<&Path>,
    skin: Option<([u32; 2], Vec<u8>)>,
) -> Result<PreviewModel, String> {
    let cant = |e: &dyn std::fmt::Display| {
        log::warn!("cpm: preview {name}: {e}");
        CANT_READ.to_owned()
    };
    let desc = cpm_core::format::read_cpmmodel(bytes).map_err(|e| cant(&e))?.desc;
    let mut model = match cpm_core::load(bytes) {
        Ok(model) => model,
        Err(cpm_core::CpmError::NeedsLink(_)) => {
            let read = |link: &str| {
                let cache = data_dir.and_then(|d| cache_path(d, link)).filter(|c| c.is_file());
                let Some(cache) = cache else { return Err(cpm_core::CpmError::NeedsLink(link.to_owned())) };
                std::fs::read(&cache).map_err(|e| cpm_core::CpmError::Invalid(format!("cannot read the cache: {e}")))
            };
            match cpm_core::link::load_with_links(bytes, &read) {
                Ok(model) => model,
                Err(cpm_core::CpmError::NeedsLink(_)) => return Err(NOT_CACHED.to_owned()),
                Err(e) => return Err(cant(&e)),
            }
        }
        Err(e) => return Err(cant(&e)),
    };
    if let Some((size, rgba)) = skin {
        cpm_core::skin::use_skin_texture(&mut model, size, rgba);
    }
    check_texture(name, &model).map_err(|e| cant(&e))?;
    let old_conversion = converted_version(&desc).is_some_and(|v| v < CONVERTER_VERSION);
    Ok(PreviewModel { fit: rest_fit(&model), live_fit: None, model, old_conversion })
}

/// What a picker file's read gave: its icon at [`ICON_PX`] a side, or why it does not read.
#[derive(Clone)]
struct TileScan {
    icon: Option<Vec<u8>>,
    error: Option<String>,
}

/// Reads `path`'s icon for its tile.
fn scan_tile(path: &Path) -> TileScan {
    let file = std::fs::read(path).ok().and_then(|b| cpm_core::format::read_cpmmodel(&b).ok());
    match file {
        Some(f) => TileScan { icon: cpm_core::mesh::icon_rgba(&f).map(|(w, h, rgba)| icon_tile(w, h, &rgba)), error: None },
        None => TileScan { icon: None, error: Some(CANT_READ.to_owned()) },
    }
}

/// `rgba` (`w` x `h`) scaled to [`ICON_PX`] a side (nearest pixel).
fn icon_tile(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity((ICON_PX * ICON_PX * 4) as usize);
    for y in 0..ICON_PX {
        let sy = (y * h / ICON_PX).min(h - 1);
        for x in 0..ICON_PX {
            let sx = (x * w / ICON_PX).min(w - 1);
            let at = ((sy * w + sx) * 4) as usize;
            out.extend_from_slice(&rgba[at..at + 4]);
        }
    }
    out
}

/// The picker's preview.
enum Preview {
    Nothing,
    /// Its file is being read (by job [`next_job`]).
    Loading(u64),
    Ready(Box<PreviewModel>),
    Failed(String),
}

/// The picker on screen.
struct Picker {
    /// The `player_models` files, read when it opened.
    files: Vec<PathBuf>,
    /// Their reads, once the job reading them (`scan_job`) ended; empty until then.
    tiles: Vec<TileScan>,
    scan_job: Option<u64>,
    page: usize,
    /// The file clicked (previewed).
    chosen: Option<usize>,
    preview: Preview,
}

/// What [`picker_preview_status`] says of the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewStatus {
    /// No tile clicked.
    Nothing,
    Loading,
    /// [`picker_preview_quads`] draws it.
    Ready,
    /// Not drawn, and why ([`TOO_LARGE`], [`CANT_READ`], [`NOT_CACHED`]).
    Message(String),
}

/// A picker tile as the screen draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    /// The file's stem.
    pub name: String,
    /// [`CANT_READ`] when it does not read.
    pub error: Option<String>,
    /// Its icon's square in [`SPRITE`], once there; `None`: a name tile.
    pub icon_at: Option<[u32; 2]>,
    /// Its file is still being read.
    pub scanning: bool,
}

/// What a picker job ended with.
enum JobOutcome {
    Tiles(Vec<TileScan>),
    Preview(Result<Box<PreviewModel>, String>),
}

struct PickerJobs {
    /// Ended jobs not yet taken.
    done: Vec<(u64, JobOutcome)>,
    /// Jobs no longer wanted: their outcome is dropped when they end.
    dropped: Vec<u64>,
}

static PICKER_JOBS: std::sync::Mutex<PickerJobs> =
    std::sync::Mutex::new(PickerJobs { done: Vec::new(), dropped: Vec::new() });
static NEXT_JOB: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn picker_jobs<R>(f: impl FnOnce(&mut PickerJobs) -> R) -> R {
    // A panic while holding it leaves plain data: keep going with it.
    f(&mut PICKER_JOBS.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}

fn next_job() -> u64 {
    NEXT_JOB.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Runs `work` on a thread of its own as job `job`; a panic (or no thread) ends it with `failed`.
fn spawn_job(job: u64, work: impl FnOnce() -> JobOutcome + Send + 'static, failed: fn() -> JobOutcome) {
    let finish = move |outcome: JobOutcome| {
        picker_jobs(|j| {
            if let Some(at) = j.dropped.iter().position(|d| *d == job) {
                j.dropped.swap_remove(at);
            } else {
                j.done.push((job, outcome));
            }
        })
    };
    let run = move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|_| {
            log::warn!("cpm: reading a model for the picker crashed");
            failed()
        });
        finish(outcome);
    };
    if let Err(e) = std::thread::Builder::new().name("cpm-picker".into()).spawn(run) {
        log::warn!("cpm: cannot start the picker's reader: {e}");
        picker_jobs(|j| j.done.push((job, failed())));
    }
}

/// Job `job`'s outcome, once.
fn take_job(job: u64) -> Option<JobOutcome> {
    picker_jobs(|j| {
        let at = j.done.iter().position(|(id, _)| *id == job)?;
        Some(j.done.swap_remove(at).1)
    })
}

/// Job `job` is no longer wanted: its outcome is dropped.
fn drop_job(job: u64) {
    picker_jobs(|j| {
        if let Some(at) = j.done.iter().position(|(id, _)| *id == job) {
            j.done.swap_remove(at);
        } else {
            j.dropped.push(job);
        }
    })
}

/// Opens the picker on the `*.cpmmodel` files of `dir` (read again now), their icons read on a
/// thread of their own. The skin's model comes first while the skin holds one ([`model_cycle`]):
/// a name tile, [`CANT_READ`] when its data does not read.
pub fn open_picker(dir: &Path) {
    let files = model_cycle(list_models(dir));
    let job = next_job();
    let scan = files.clone();
    let skin_tile = STATE.with_borrow_mut(|s| {
        drop_picker(s);
        s.picker = Some(Picker { files, tiles: Vec::new(), scan_job: Some(job), page: 0, chosen: None, preview: Preview::Nothing });
        recompose(s);
        let error = s.skin.as_ref().and_then(|k| k.model.as_ref()).is_some_and(|m| m.is_err());
        TileScan { icon: None, error: error.then(|| CANT_READ.to_owned()) }
    });
    let tiles = move || scan.iter().map(|f| if is_skin(f) { skin_tile.clone() } else { scan_tile(f) }).collect();
    spawn_job(job, move || JobOutcome::Tiles(tiles()), || JobOutcome::Tiles(Vec::new()));
}

/// Drops the picker and its jobs (the image is made again without it).
fn drop_picker(s: &mut State) {
    // The picker and the gestures page share the preview's turn and zoom: each page starts at
    // the default view, so a zoom from one page does not carry into the other.
    if let Some(m) = s.menu.as_mut() {
        m.preview = PreviewView::default();
    }
    let Some(p) = s.picker.take() else { return };
    if let Some(job) = p.scan_job {
        drop_job(job);
    }
    if let Preview::Loading(job) = p.preview {
        drop_job(job);
    }
    recompose(s);
}

/// `Back`: closes the picker (the gestures page shows again).
pub fn close_picker() {
    STATE.with_borrow_mut(drop_picker);
}

/// Whether the picker is shown (the gesture menu's screen shows it instead of the gestures).
pub fn picker_shown() -> bool {
    STATE.with_borrow(|s| s.picker.is_some())
}

/// Previews file `i` (a tile was clicked): reads it on a thread of its own. The same file again
/// does nothing.
pub fn preview_select(i: usize) {
    let job = next_job();
    let start = STATE.with_borrow_mut(|s| {
        let data_dir = s.data_dir.clone();
        let p = s.picker.as_mut()?;
        let path = p.files.get(i)?.clone();
        if p.chosen == Some(i) {
            return None;
        }
        p.chosen = Some(i);
        // A different model starts at the default turn and zoom.
        if let Some(m) = s.menu.as_mut() {
            m.preview = PreviewView::default();
        }
        let p = s.picker.as_mut()?;
        if let Preview::Loading(old) = std::mem::replace(&mut p.preview, Preview::Loading(job)) {
            drop_job(old);
        }
        recompose(s);
        // The skin's model reads from the skin, not a file.
        let skin = is_skin(&path).then(|| s.skin.as_ref().map(|k| (k.model.clone(), k.texture.clone())));
        Some((path, data_dir, skin))
    });
    let Some((path, data_dir, skin)) = start else { return };
    spawn_job(
        job,
        move || {
            JobOutcome::Preview(
                match skin {
                    None => load_preview(&path, data_dir.as_deref()),
                    Some(Some((Some(Ok(bytes)), texture))) => preview_bytes(SKIN_NAME, &bytes, data_dir.as_deref(), texture),
                    Some(_) => Err(CANT_READ.to_owned()),
                }
                .map(Box::new),
            )
        },
        || JobOutcome::Preview(Err(CANT_READ.to_owned())),
    );
}

/// Takes the previewed file's read once it ended; true when it did (the image is made again).
pub fn take_preview_result() -> bool {
    STATE.with_borrow_mut(|s| {
        let Some(Preview::Loading(job)) = s.picker.as_ref().map(|p| &p.preview) else { return false };
        let Some(JobOutcome::Preview(result)) = take_job(*job) else { return false };
        if let Some(p) = s.picker.as_mut() {
            p.preview = match result {
                Ok(m) => Preview::Ready(m),
                Err(e) => Preview::Failed(e),
            };
        }
        recompose(s);
        true
    })
}

/// Takes what the picker's jobs ended with (the icons, the previewed model); true when anything
/// changed. Polled once a frame, before [`texture_upload`].
pub fn poll_picker() -> bool {
    let tiles = STATE.with_borrow_mut(|s| {
        let Some(job) = s.picker.as_ref().and_then(|p| p.scan_job) else { return false };
        let Some(JobOutcome::Tiles(tiles)) = take_job(job) else { return false };
        if let Some(p) = s.picker.as_mut() {
            p.scan_job = None;
            p.tiles = tiles;
        }
        recompose(s);
        true
    });
    take_preview_result() | tiles
}

/// The picker's tiles, in file order (none when it is not shown).
pub fn picker_tiles() -> Vec<Tile> {
    STATE.with_borrow(|s| {
        let Some(p) = s.picker.as_ref() else { return Vec::new() };
        let placed = s.composed.as_ref().map(|c| c.icons.as_slice()).unwrap_or_default();
        p.files
            .iter()
            .enumerate()
            .map(|(i, f)| Tile {
                name: if is_skin(f) {
                    SKIN_MODEL.to_owned()
                } else {
                    f.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                },
                error: p.tiles.get(i).and_then(|t| t.error.clone()),
                icon_at: placed.get(i).copied().flatten(),
                scanning: p.scan_job.is_some(),
            })
            .collect()
    })
}

/// The picker's page and the file clicked.
pub fn picker_page() -> (usize, Option<usize>) {
    STATE.with_borrow(|s| s.picker.as_ref().map_or((0, None), |p| (p.page, p.chosen)))
}

/// Shows page `page` of the picker.
pub fn set_picker_page(page: usize) {
    STATE.with_borrow_mut(|s| {
        if let Some(p) = s.picker.as_mut() {
            p.page = page;
        }
    });
}

/// The picker's preview: what it shows, or why not.
pub fn picker_preview_status() -> PreviewStatus {
    STATE.with_borrow(|s| {
        let Some(p) = s.picker.as_ref() else { return PreviewStatus::Nothing };
        match &p.preview {
            Preview::Nothing => PreviewStatus::Nothing,
            Preview::Loading(_) => PreviewStatus::Loading,
            Preview::Failed(e) => PreviewStatus::Message(e.clone()),
            Preview::Ready(m) => {
                let active = s.model.as_ref().map_or([0, 0], |a| model_composite(a).size);
                if slot_layout(active, Some(model_composite(&m.model).size), 0).preview_at.is_none() {
                    PreviewStatus::Message(TOO_LARGE.to_owned())
                } else if m.fit.is_none() {
                    PreviewStatus::Message(CANT_READ.to_owned())
                } else {
                    PreviewStatus::Ready
                }
            }
        }
    })
}

/// The previewed model's note: the settings page's [`old_conversion_note`] when an older converter
/// made it.
pub fn picker_note() -> Option<String> {
    STATE.with_borrow(|s| {
        let p = s.picker.as_ref()?;
        let Preview::Ready(m) = &p.preview else { return None };
        let stem = p.files.get(p.chosen?)?.file_stem()?.to_string_lossy().into_owned();
        m.old_conversion.then(|| old_conversion_note(&stem))
    })
}

/// The previewed model at `now_ms`, standing, at its default parameters, wearing `worn`, as the
/// preview in `rect` draws it (its [`rest_fit`]); `None` while its image has not reached the game
/// (or has no room: [`TOO_LARGE`]), or with nothing previewed.
pub fn picker_preview_quads(worn: &Worn, view: &PreviewView, rect: [f32; 4], now_ms: u64) -> Option<PreviewQuads> {
    STATE.with_borrow_mut(|s| {
        let at = s.composed.as_ref()?.layout.preview_at?;
        let Preview::Ready(p) = &mut s.picker.as_mut()?.preview else { return None };
        p.fit?;
        let m = &mut p.model;
        let equipment = worn.equipment(elytra_sheet(m).is_some());
        // The cape hangs still (no motion to read for a model not worn).
        let cape = Some(cpm_core::CapeState::default());
        let input = PoseInput { time_ms: now_ms, on_ground: true, equipment, cape, ..PoseInput::default() };
        let boxes = m.pose(&input);
        // As for the loaded model ([`preview_quads`]): fitted to the first frame it draws.
        let fit = match p.live_fit {
            Some(fit) => fit,
            None => {
                let fit = preview_fit(&boxes)?;
                *p.live_fit.insert(fit)
            }
        };
        Some(preview_box_quads_at(&p.model, &boxes, worn, &fit, view, rect, at))
    })
}

/// `Use this model`: selects the clicked file and closes the picker; its file name, to keep as the
/// [`MODEL_SETTING`] (the value the settings page's Model button keeps: [`model_setting`]). The
/// selection then loads as any does ([`ensure_loaded`], its kept parameters restored). `None` (and
/// nothing changes) when no file was clicked. A link model not downloaded yet downloads now.
pub fn use_previewed() -> Option<String> {
    STATE.with_borrow_mut(|s| {
        let p = s.picker.as_ref()?;
        let path = p.files.get(p.chosen?)?.clone();
        drop_picker(s);
        let value = model_setting(Some(&path));
        s.selected = Some(path);
        Some(value)
    })
}

/// The mod setting that keeps a model's parameters: `params.<model file name>`.
pub fn params_setting_key(model_file: &str) -> String {
    format!("params.{model_file}")
}

/// Parameter bytes as lowercase hex.
pub fn encode_params(p: &[u8]) -> String {
    use std::fmt::Write;
    p.iter().fold(String::with_capacity(p.len() * 2), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// Parameter bytes from hex (either case); `None` for an odd length or a non-hex character.
pub fn decode_params(text: &str) -> Option<Vec<u8>> {
    if !text.bytes().all(|b| b.is_ascii_hexdigit()) || text.len() % 2 != 0 {
        return None;
    }
    (0..text.len() / 2).map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()).collect()
}

/// Right after a model finished loading (once per load): gives it the parameters kept for its
/// file. `get` reads a mod setting by key. Nothing kept, bad hex or a length that is not the
/// model's (it changed since) leave the defaults.
pub fn restore_params(get: impl Fn(&str) -> Option<String>) {
    STATE.with_borrow_mut(|s| {
        if !std::mem::take(&mut s.restore_pending) {
            return;
        }
        let name = file_name(s.loaded.as_deref());
        let Some(m) = s.model.as_mut() else { return };
        if let Some(p) = get(&params_setting_key(&name)).and_then(|t| decode_params(&t)) {
            m.engine.set_parameters(&p);
        }
        s.params_saved = Some((name, m.engine.parameters()));
    });
}

/// The parameters to keep, if they changed since they were restored or last returned here:
/// (setting key, hex value) for `native_set_setting`. Polled once a frame.
pub fn take_params_to_save() -> Option<(String, String)> {
    STATE.with_borrow_mut(|s| {
        // A slider drag changes the parameters every frame: save once, after it ends.
        if s.menu.as_ref().is_some_and(|m| m.dragging.is_some()) {
            return None;
        }
        let name = file_name(s.loaded.as_deref());
        let now = s.model.as_ref()?.engine.parameters();
        let (saved_name, saved) = s.params_saved.as_ref()?;
        if *saved_name == name && *saved == now {
            return None;
        }
        let out = (params_setting_key(&name), encode_params(&now));
        s.params_saved = Some((name, now));
        Some(out)
    })
}

/// What the settings page shows of the converter.
#[derive(Debug, Clone, Copy)]
pub struct ConvertView<'a> {
    /// A conversion (or its file picker) is running.
    pub busy: bool,
    /// The last conversion's outcome.
    pub last: Option<&'a ConvertResult>,
    /// The `gist_limit_kib` setting ([`gist_limit`]).
    pub limit_kib: u32,
    /// The selected model was made by an older converter ([`old_conversion`]).
    pub old_conversion: bool,
}

/// The settings page: the model (button 0), the folder (button 1), `Convert project…` (button 2),
/// the gist limit (button 3), `Gestures…` (button 4), then [`SKIN_IN_USE`] when `selected` (the
/// [`active_selection`]) is the skin's model, the last conversion's outcome and the model's error
/// if any.
pub fn settings_labels(
    selected: Option<&Path>,
    error: Option<&str>,
    convert: &ConvertView<'_>,
) -> Vec<String> {
    let skin = selected.is_some_and(is_skin);
    let model = if skin {
        SKIN_MODEL.to_owned()
    } else {
        selected.and_then(Path::file_stem).map_or_else(|| "None".to_owned(), |s| s.to_string_lossy().into_owned())
    };
    let mut labels = vec![
        format!("Model: {model}"),
        "Open models folder".to_owned(),
        if convert.busy { "Converting…" } else { "Convert project…" }.to_owned(),
        format!("Gist limit: {} KiB", convert.limit_kib),
        "Gestures…".to_owned(),
    ];
    if skin {
        labels.push(SKIN_IN_USE.to_owned());
    }
    labels.extend(convert.last.map(convert_label));
    if convert.old_conversion {
        labels.push(old_conversion_note(&model));
    }
    labels.extend(error.map(|e| format!("Error: {e}")));
    labels
}

/// What a click on the settings page does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Click {
    /// Select this model (`None`: the vanilla player) and keep it.
    Select(Option<PathBuf>),
    OpenFolder,
    /// Pick a `.cpmproject` and convert it (nothing while a conversion runs).
    Convert,
    /// Store the next gist limit ([`next_gist_limit`]).
    GistLimit,
    /// Open the gesture menu ([`request_menu`]).
    Gestures,
    Nothing,
}

/// A click on button `button`: the model button cycles None, first, ..., last, None.
pub fn click(button: usize, models: &[PathBuf], selected: Option<&Path>) -> Click {
    match button {
        0 => {
            let at = selected.and_then(|s| models.iter().position(|m| m == s));
            Click::Select(match at {
                Some(i) => models.get(i + 1).cloned(),
                None => models.first().cloned(),
            })
        }
        1 => Click::OpenFolder,
        2 => Click::Convert,
        3 => Click::GistLimit,
        4 => Click::Gestures,
        _ => Click::Nothing,
    }
}

/// Opens `dir` (made when missing) in the system's file browser; failures are logged.
pub fn open_folder(dir: &Path) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!("cpm: cannot make {}: {e}", dir.display());
        return;
    }
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(dir).spawn() {
        log::warn!("cpm: cannot open {} with {program}: {e}", dir.display());
    }
}

// ---- Converting a .cpmproject (the settings page's `Convert project…`) ----
// The picker and the conversion run on their own thread (convert.rs, `game::cpm_convert`); the
// outcome crosses back through a process-wide `Mutex` ([`finish_convert_job`]), not `State`.

/// What a conversion did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertResult {
    /// The model was written to `model` (`size` bytes). `too_big`: larger than the gist limit, so
    /// `<stem>.gist.txt` ([`gist_path`]) was written next to it for a gist link. `skipped`: how many
    /// of the project's features were not converted (each logged, `cpm: convert <file>: <what>`).
    Saved { model: PathBuf, too_big: bool, size: usize, skipped: usize },
    /// Nothing was written; why.
    Failed(String),
}

/// The converter's version, written into every model's description. Version 1 (no number in the
/// marker) lacked the armour parts; version 2 the render scale and the first-person hand;
/// version 3 animated textures.
pub const CONVERTER_VERSION: u32 = 4;

/// Ends the description of every version 1 model; later ones end `, v<N>)` instead.
pub const CONVERTED_MARKER: &str = " (converted by CPM for Horizon)";

/// The version of the converter that wrote a model with description `desc`: 1 for the old
/// [`CONVERTED_MARKER`], N for a description ending ` (converted by CPM for Horizon, vN)`, `None`
/// for a model the converter did not write. [`output_path`] uses it to tell its own earlier
/// output (overwritten) from someone else's model (kept).
pub fn converted_version(desc: &str) -> Option<u32> {
    if desc.ends_with(CONVERTED_MARKER) {
        return Some(1);
    }
    let head = desc.strip_suffix(')')?;
    let (head, n) = head.rsplit_once(", v")?;
    if !head.ends_with(" (converted by CPM for Horizon") || n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}

/// The setting the gist limit (KiB) is kept under.
pub const GIST_LIMIT_SETTING: &str = "gist_limit_kib";

/// The stored gist limit in KiB: 8 when unset or not a number, else clamped to 1..=1024.
pub fn gist_limit(setting: Option<String>) -> u32 {
    setting.and_then(|s| s.trim().parse::<u64>().ok()).map_or(8, |n| n.clamp(1, 1024) as u32)
}

/// The gist limit button's next value: 4, 8, 16, 32, 64, then 4 again (anything else: 4).
pub fn next_gist_limit(kib: u32) -> u32 {
    match kib {
        4 | 8 | 16 | 32 => kib * 2,
        _ => 4,
    }
}

/// The gist text file written next to a too-big `model`: `<stem>.gist.txt`.
pub fn gist_path(model: &Path) -> PathBuf {
    model.with_extension("gist.txt")
}

/// Whether `path` is a model the converter wrote (see [`converted_version`]).
fn is_converted(path: &Path) -> bool {
    std::fs::read(path)
        .ok()
        .and_then(|b| cpm_core::format::read_cpmmodel(&b).ok())
        .is_some_and(|m| converted_version(&m.desc).is_some())
}

/// Where `project` is converted to in `models`: `<stem>.cpmmodel`, or, when that file exists and
/// is not one the converter wrote, the first of `<stem>_2.cpmmodel`, `<stem>_3.cpmmodel`, ... that
/// is free or the converter's.
pub fn output_path(models: &Path, project: &Path) -> PathBuf {
    let stem = project.file_stem().map_or_else(|| "model".to_owned(), |s| s.to_string_lossy().into_owned());
    (1u32..)
        .map(|n| if n == 1 { format!("{stem}.cpmmodel") } else { format!("{stem}_{n}.cpmmodel") })
        .map(|name| models.join(name))
        .find(|path| !path.exists() || is_converted(path))
        .expect("an unbounded search")
}

/// Converts the `.cpmproject` at `project` into `models` ([`output_path`]), its description
/// ending with the converter marker for [`CONVERTER_VERSION`]. Bigger than `limit_kib` KiB: also writes the gist text
/// ([`gist_path`]). The features it skips are logged (`cpm: convert <file>: <what>`) and counted.
/// On any failure, an exported model that does not load ([`check_export`]) included, nothing is
/// written (`Failed`, also logged).
pub fn convert_project(project: &Path, models: &Path, limit_kib: u32) -> ConvertResult {
    let name = file_name(Some(project));
    let result = (|| {
        let bytes = std::fs::read(project).map_err(|e| e.to_string())?;
        let mut parsed = cpm_core::project::read_project(&bytes).map_err(|e| e.to_string())?;
        parsed.desc.push_str(&format!(" (converted by CPM for Horizon, v{CONVERTER_VERSION})"));
        let model = cpm_core::export::export_model(&parsed).map_err(|e| e.to_string())?;
        check_export(&model)?;
        let too_big = model.len() > limit_kib as usize * 1024;
        let gist = if too_big { Some(cpm_core::link::gist_text(&model).map_err(|e| e.to_string())?) } else { None };
        std::fs::create_dir_all(models).map_err(|e| format!("cannot make {}: {e}", models.display()))?;
        let path = output_path(models, project);
        let gist_file = gist_path(&path);
        // Both files are written beside their places first, then renamed into them (the gist
        // text first): a failure leaves the files already there untouched.
        let model_tmp = staged(&path, &model)?;
        if let Some(text) = &gist {
            let gist_tmp = staged(&gist_file, text.as_bytes()).inspect_err(|_| {
                let _ = std::fs::remove_file(&model_tmp);
            })?;
            if let Err(e) = std::fs::rename(&gist_tmp, &gist_file) {
                let _ = std::fs::remove_file(&gist_tmp);
                let _ = std::fs::remove_file(&model_tmp);
                return Err(format!("cannot write {}: {e}", file_name(Some(&gist_file))));
            }
        }
        if let Err(e) = std::fs::rename(&model_tmp, &path) {
            let _ = std::fs::remove_file(&model_tmp);
            return Err(format!("cannot write {}: {e}", file_name(Some(&path))));
        }
        if gist.is_none() {
            // An earlier, bigger conversion's gist text no longer matches the model.
            let _ = std::fs::remove_file(&gist_file);
        }
        for w in &parsed.warnings {
            log::warn!("cpm: convert {name}: {w}");
        }
        Ok(ConvertResult::Saved { model: path, too_big, size: model.len(), skipped: parsed.warnings.len() })
    })();
    result.unwrap_or_else(|e: String| {
        let error = format!("{name}: {e}");
        log::warn!("cpm: convert {error}");
        ConvertResult::Failed(error)
    })
}

/// Loads an exported model back (as the game will): `exported model does not load: <why>` when
/// it does not, and the converter then writes nothing.
pub fn check_export(model: &[u8]) -> Result<(), String> {
    cpm_core::load(model).map(drop).map_err(|e| format!("exported model does not load: {e}"))
}

/// Writes `bytes` to `<path>.tmp` (removed again on failure) for renaming into `path`.
fn staged(path: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("cannot write {}: {e}", file_name(Some(path)))
    })?;
    Ok(tmp)
}

/// The settings page's line for a conversion's outcome.
pub fn convert_label(result: &ConvertResult) -> String {
    match result {
        ConvertResult::Saved { model, too_big, size, skipped } => {
            let mut label = if *too_big {
                format!("Converted: {} — too big: the gist popup opens when you return to the game", file_name(Some(model)))
            } else {
                format!("Converted: {} ({} KiB)", file_name(Some(model)), size.div_ceil(1024))
            };
            if *skipped > 0 {
                label.push_str(&format!(" ({skipped} skipped, see log)"));
            }
            label
        }
        ConvertResult::Failed(error) => format!("Convert error: {error}"),
    }
}

struct Job {
    busy: bool,
    result: Option<ConvertResult>,
    /// How many conversions (or cancelled pickers) have ended.
    finished: u64,
}

static JOB: std::sync::Mutex<Job> = std::sync::Mutex::new(Job { busy: false, result: None, finished: 0 });

fn job<R>(f: impl FnOnce(&mut Job) -> R) -> R {
    // A panic while holding it leaves plain data: keep going with it.
    f(&mut JOB.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}

/// Marks a conversion as running; false (nothing changes) when one already is.
pub fn start_convert_job() -> bool {
    job(|j| !std::mem::replace(&mut j.busy, true))
}

/// The running conversion ended (from any thread): `None` when the picker was cancelled.
pub fn finish_convert_job(r: Option<ConvertResult>) {
    job(|j| {
        j.busy = false;
        j.finished += 1;
        if r.is_some() {
            j.result = r;
        }
    });
}

/// The finished conversion's outcome, once.
pub fn take_convert_result() -> Option<ConvertResult> {
    job(|j| j.result.take())
}

/// A conversion (or its picker) is running.
pub fn convert_busy() -> bool {
    job(|j| j.busy)
}

/// Keeps `result` for the settings page; a too-big model also becomes the pending gist popup. A
/// model written over the selected one is loaded again (its texture uploaded again).
pub fn note_convert_result(result: ConvertResult) {
    STATE.with_borrow_mut(|s| {
        if let ConvertResult::Saved { model, too_big, size, .. } = &result {
            if *too_big {
                s.gist_popup = Some((model.clone(), *size));
            }
            if s.selected.as_ref() == Some(model) {
                s.loaded = None;
                s.uploaded = None;
            }
        }
        s.last_convert = Some(result);
    });
}

/// Moves a finished conversion's outcome ([`take_convert_result`]) into this thread's state
/// ([`note_convert_result`]). True when a conversion (or a cancelled picker) ended since the last
/// call on this thread: the settings page still shows `Converting…` then and needs rebuilding. The
/// frame, settings and click helpers call it.
pub fn poll_convert() -> bool {
    // The count first: a job ending in between is noted now and counted at the next call.
    let finished = job(|j| j.finished);
    if let Some(result) = take_convert_result() {
        note_convert_result(result);
    }
    STATE.with_borrow_mut(|s| std::mem::replace(&mut s.convert_seen, finished) != finished)
}

/// The last conversion's outcome noted on this thread.
pub fn last_convert() -> Option<ConvertResult> {
    STATE.with_borrow(|s| s.last_convert.clone())
}

/// The too-big model (and its size) whose gist popup is due.
pub fn pending_gist_popup() -> Option<(PathBuf, usize)> {
    STATE.with_borrow(|s| s.gist_popup.clone())
}

/// Takes the due gist popup: it is shown once.
pub fn take_gist_popup() -> Option<(PathBuf, usize)> {
    STATE.with_borrow_mut(|s| s.gist_popup.take())
}

// ---- The gist popup: a mod screen (gist_screen.rs draws it and feeds it input) ----

/// The popup's title.
pub const GIST_TITLE: &str = "Upload to a GitHub Gist";
/// What [`GistPopup::submit`] says of a URL that is not a gist page.
pub const NOT_A_GIST: &str = "Not a gist link: use https://gist.github.com/<user>/<id>";
/// The longest URL the field takes, in characters.
const URL_MAX_CHARS: usize = 256;

/// The gist popup for a converted model too big to send directly: the gist link being typed
/// (the cursor always at its end) and the last thing a button said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GistPopup {
    /// The full converted model (`<stem>.cpmmodel`); its gist text is [`gist_path`].
    pub model: PathBuf,
    /// Its size in bytes.
    pub size: usize,
    /// The gist link typed or pasted so far.
    pub url: String,
    /// The caret, a byte index into `url` (always its end).
    pub cursor: usize,
    /// The last button's outcome or error.
    pub message: Option<String>,
}

impl GistPopup {
    pub fn new(model: PathBuf, size: usize) -> Self {
        GistPopup { model, size, url: String::new(), cursor: 0, message: None }
    }

    /// Types `c` at the end (control characters and past [`URL_MAX_CHARS`]: nothing).
    pub fn type_char(&mut self, c: char) {
        if !c.is_control() && self.url.chars().count() < URL_MAX_CHARS {
            self.url.push(c);
        }
        self.cursor = self.url.len();
    }

    /// Removes the last character.
    pub fn backspace(&mut self) {
        self.url.pop();
        self.cursor = self.url.len();
    }

    /// Appends `text` (trimmed, without control characters such as line breaks).
    pub fn paste(&mut self, text: &str) {
        for c in text.trim().chars() {
            self.type_char(c);
        }
    }

    fn stem(&self) -> String {
        self.model.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// Writes the link model for the typed gist link — `<stem>.link.cpmmodel` next to the model,
    /// named `name`, described `desc`, of `skin_type` — and returns its path. A URL that is not a
    /// gist page: [`NOT_A_GIST`], nothing written.
    pub fn submit(&self, name: &str, desc: &str, skin_type: cpm_core::format::SkinType) -> Result<PathBuf, String> {
        let link = cpm_core::link::parse_gist_url(&self.url).ok_or_else(|| NOT_A_GIST.to_owned())?;
        let path = self.model.with_extension("link.cpmmodel");
        let bytes = cpm_core::link::link_model(name, desc, skin_type, &link);
        let tmp = staged(&path, &bytes)?;
        std::fs::rename(&tmp, &path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("cannot write {}: {e}", file_name(Some(&path)))
        })?;
        Ok(path)
    }

    /// `Save link`: [`submit`](Self::submit) with the converted model's own name, description
    /// (the converter's marker included) and skin type; the outcome becomes the message.
    pub fn save_link(&mut self) -> Result<PathBuf, String> {
        let result = (|| {
            if cpm_core::link::parse_gist_url(&self.url).is_none() {
                return Err(NOT_A_GIST.to_owned());
            }
            let bytes = std::fs::read(&self.model).map_err(|e| format!("{}: {e}", file_name(Some(&self.model))))?;
            let full = cpm_core::format::read_cpmmodel(&bytes).map_err(|e| format!("{}: {e}", file_name(Some(&self.model))))?;
            self.submit(&full.name, &full.desc, full.data.skin_type)
        })();
        self.message = Some(match &result {
            Ok(path) => {
                log::info!("cpm: saved the gist link model {}", path.display());
                format!("Saved {}", file_name(Some(path)))
            }
            Err(e) => e.clone(),
        });
        result
    }
}

/// The popup's lines under its title.
pub fn popup_lines(p: &GistPopup) -> Vec<String> {
    let gist = file_name(Some(&gist_path(&p.model)));
    vec![
        format!("{} is {} KiB, too big to send directly.", file_name(Some(&p.model)), p.size.div_ceil(1024)),
        format!("1. Press Copy text (also saved as {gist})."),
        "2. Create a public gist with it at gist.github.com.".to_owned(),
        "3. Paste the gist link here and press Save link.".to_owned(),
    ]
}

/// `Copy text`'s message when the clipboard could not take the text.
pub fn copy_failed_message(p: &GistPopup) -> String {
    format!("Couldn't copy — open {}.gist.txt instead", p.stem())
}

/// The popup's buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GistButton {
    Copy,
    Open,
    Save,
    Close,
}

/// The buttons and their labels, in [`GistLayout::buttons`] order.
pub const GIST_BUTTONS: [(GistButton, &str); 4] = [
    (GistButton::Copy, "Copy text"),
    (GistButton::Open, "Open gist.github.com"),
    (GistButton::Save, "Save link"),
    (GistButton::Close, "Close"),
];

/// Where the popup's parts are, in GUI pixels (`[x, y, w, h]`; text rows by their top `y`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GistLayout {
    pub panel: [f32; 4],
    pub title_y: f32,
    /// The first of [`popup_lines`]; each next one [`GIST_LINE_H`] lower.
    pub lines_y: f32,
    pub field: [f32; 4],
    pub message_y: f32,
    pub buttons: [[f32; 4]; 4],
}

/// A text row's height.
pub const GIST_LINE_H: f32 = 11.0;
/// Space around the panel's content.
pub const GIST_PAD: f32 = 8.0;

/// The popup centred in `window` (the screen's rect), at most 420 wide; buttons two by two.
pub fn gist_layout(window: [f32; 4]) -> GistLayout {
    let [wx, wy, ww, wh] = window;
    let (button_h, gap) = (20.0, 4.0);
    let width = (ww - 16.0).clamp(160.0, 420.0);
    let title_y = GIST_PAD;
    let lines_y = title_y + 16.0;
    let field_y = lines_y + 4.0 * GIST_LINE_H + 4.0;
    let field_h = 14.0;
    let message_y = field_y + field_h + 4.0;
    let buttons_y = message_y + GIST_LINE_H + 4.0;
    let height = buttons_y + 2.0 * button_h + gap + GIST_PAD;
    let (x, y) = (wx + ((ww - width) / 2.0).max(0.0), wy + ((wh - height) / 2.0).max(0.0));
    let inner = width - 2.0 * GIST_PAD;
    let bw = (inner - gap) / 2.0;
    let button = |i: usize| {
        let (col, row) = ((i % 2) as f32, (i / 2) as f32);
        [x + GIST_PAD + col * (bw + gap), y + buttons_y + row * (button_h + gap), bw, button_h]
    };
    GistLayout {
        panel: [x, y, width, height],
        title_y: y + title_y,
        lines_y: y + lines_y,
        field: [x + GIST_PAD, y + field_y, inner, field_h],
        message_y: y + message_y,
        buttons: [button(0), button(1), button(2), button(3)],
    }
}

/// Whether `at` is inside `rect`.
pub fn inside(rect: [f32; 4], at: [f32; 2]) -> bool {
    let [x, y, w, h] = rect;
    at[0] >= x && at[0] < x + w && at[1] >= y && at[1] < y + h
}

/// The button under `at`, if any.
pub fn gist_button_at(layout: &GistLayout, at: [f32; 2]) -> Option<GistButton> {
    GIST_BUTTONS.iter().zip(layout.buttons).find(|(_, rect)| inside(*rect, at)).map(|((b, _), _)| *b)
}

/// What a key pressed on the popup does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GistKey {
    Backspace,
    /// Enter: `Save link`.
    Save,
    /// Ctrl+V (Cmd+V): paste into the field.
    Paste,
    /// Escape: left to the game, which closes the screen.
    Pass,
    /// Anything else: kept from the game (no inventory key closing the popup mid-typing).
    Swallow,
}

/// GLFW modifier bits: control, super.
const CONTROL: u32 = 2;
const SUPER: u32 = 8;

/// The popup's meaning of `key` (vanilla's key name) with GLFW `modifiers`.
pub fn gist_key(key: &str, modifiers: u32) -> GistKey {
    match key {
        "key.keyboard.escape" => GistKey::Pass,
        "key.keyboard.backspace" => GistKey::Backspace,
        "key.keyboard.enter" | "key.keyboard.keypad.enter" => GistKey::Save,
        "key.keyboard.v" if modifiers & (CONTROL | SUPER) != 0 => GistKey::Paste,
        _ => GistKey::Swallow,
    }
}

/// What the popup's input hooks leave for the frame hook, which holds the game mutably (the
/// clipboard, closing the screen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GistRequest {
    Copy,
    Paste,
    Close,
}

/// Shows `p` (the mod screen was just opened for it).
pub fn show_gist_popup(p: GistPopup) {
    STATE.with_borrow_mut(|s| s.gist = Some(p));
}

/// Drops the popup and its pending requests (its screen closed).
pub fn hide_gist_popup() {
    STATE.with_borrow_mut(|s| {
        s.gist = None;
        s.gist_requests.clear();
    });
}

/// Whether the popup is shown.
pub fn gist_popup_shown() -> bool {
    STATE.with_borrow(|s| s.gist.is_some())
}

/// Runs `f` on the shown popup; `None` when none is.
pub fn with_gist_popup<R>(f: impl FnOnce(&mut GistPopup) -> R) -> Option<R> {
    STATE.with_borrow_mut(|s| s.gist.as_mut().map(f))
}

/// Leaves `r` for the frame hook (nothing when no popup is shown).
pub fn request_gist(r: GistRequest) {
    STATE.with_borrow_mut(|s| {
        if s.gist.is_some() {
            s.gist_requests.push(r);
        }
    });
}

/// The requests left since the last call, in order.
pub fn take_gist_requests() -> Vec<GistRequest> {
    STATE.with_borrow_mut(|s| std::mem::take(&mut s.gist_requests))
}

/// The environment variable the Windows picker reads its starting folder from (never quoted
/// into the script).
pub const PICKER_DIR_ENV: &str = "CPM_DIR";

/// The PowerShell script of the Windows file picker: a `.cpmproject` open dialog starting in the
/// folder [`PICKER_DIR_ENV`] names, over a topmost owner (so not behind the game); prints the
/// chosen file (as UTF-8), nothing when cancelled.
pub fn powershell_picker_script() -> String {
    format!(
        "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; Add-Type -AssemblyName System.Windows.Forms; \
         $d=New-Object System.Windows.Forms.OpenFileDialog; $d.Filter='CPM project (*.cpmproject)|*.cpmproject'; \
         $d.InitialDirectory=$env:{PICKER_DIR_ENV}; \
         $f=New-Object System.Windows.Forms.Form -Property @{{TopMost=$true}}; \
         if($d.ShowDialog($f) -eq 'OK'){{ $d.FileName }}; $f.Dispose()"
    )
}

/// What a picker's run means: the picked file (trimmed stdout), `None` when it printed nothing
/// (cancelled: zenity, kdialog and osascript exit non-zero then). On Windows, whose picker exits 0
/// on cancel, a failure that printed to stderr is an error: its first non-empty line.
pub fn picker_outcome(stdout: &[u8], success: bool, stderr: &[u8], windows: bool) -> Result<Option<PathBuf>, String> {
    let picked = String::from_utf8_lossy(stdout);
    let picked = picked.trim();
    if !picked.is_empty() {
        return Ok(Some(PathBuf::from(picked)));
    }
    if windows && !success {
        let stderr = String::from_utf8_lossy(stderr);
        if let Some(line) = stderr.lines().map(str::trim).find(|l| !l.is_empty()) {
            return Err(line.to_owned());
        }
    }
    Ok(None)
}
