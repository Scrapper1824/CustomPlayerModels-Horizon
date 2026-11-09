//! Links (`loader:path`: GitHub gists and the other CPM loaders) and link models.
//!
//! A *link model* is a `.cpmmodel` whose data is `SKIN_TYPE`, a `PACKAGE_LINK` and `END`
//! (`Exporter.writeOut`). The linked gist holds the base64 of the *package*: `0x53`, the
//! definition's parts (`PartCollection.writePackage`: every part but the skin type, then
//! `END`) and the 2-byte checksum.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::format::{HEADER, SkinType, read_cpmmodel, read_cpmmodel_resolving, read_definition};
use crate::io::{CpmError, Reader, Result};
use crate::mesh::LoadedModel;
use crate::write::{Writer, checksummed};

const END: u8 = 0;
const SKIN_TYPE: u8 = 11;
const PACKAGE_LINK: u8 = 20;

/// Page URL pattern of `GistResourceLoader`, also accepting the raw URL and a trailing `/raw`.
/// Returns `git:<user>/<id>`.
pub fn parse_gist_url(url: &str) -> Option<String> {
    let u = url.trim();
    let rest = u
        .strip_prefix("https://gist.github.com/")
        .or_else(|| u.strip_prefix("https://gist.githubusercontent.com/"))?;
    let rest = rest.split(['#', '?']).next()?;
    let mut it = rest.split('/');
    let user = it.next()?;
    let id = it.next()?;
    let ok = |s: &str, dash: bool| {
        !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || (dash && c == '-'))
    };
    if !ok(user, true) || !ok(id, false) {
        return None;
    }
    match (it.next(), it.next()) {
        (None, _) | (Some(""), None) | (Some("raw"), None) | (Some("raw"), Some("")) => {}
        _ => return None,
    }
    Some(format!("git:{user}/{id}"))
}

/// Raw-text URL for a `git:` link (`GistResourceLoader.createURL`).
pub fn gist_raw_url(link: &str) -> Option<String> {
    let path = link.strip_prefix("git:")?;
    let (user, id) = path.split_once('/')?;
    if user.is_empty() || id.is_empty() || id.contains('/') {
        return None;
    }
    Some(format!("https://gist.githubusercontent.com/{user}/{id}/raw"))
}

/// The data block of a `.cpmmodel` (whole file validated by [`read_cpmmodel`] first).
fn data_block(model_file: &[u8]) -> Result<&[u8]> {
    read_cpmmodel(model_file)?;
    let mut r = Reader::new(model_file);
    r.u8()?;
    r.utf()?;
    r.utf()?;
    let n = r.varint()? as usize;
    let start = r.pos();
    model_file.get(start..start + n).ok_or(CpmError::Eof)
}

/// The gist text for a `.cpmmodel`: base64 of the package form of its data block.
pub fn gist_text(model_file: &[u8]) -> Result<String> {
    let data = data_block(model_file)?;
    let mut r = Reader::new(data);
    if r.u8()? != HEADER {
        return Err(CpmError::BadHeader);
    }
    let mut body = Writer::new();
    loop {
        let ty = r.u8()?;
        let mut block = r.block()?;
        if ty == SKIN_TYPE {
            continue; // `writeOut` writes the skin type only in the outer container
        }
        let n = block.remaining();
        let mut inner = Writer::new();
        inner.bytes(block.bytes(n)?);
        body.object_block(ty, &inner);
        if ty == END {
            break;
        }
    }
    Ok(STANDARD.encode(checksummed(HEADER, &body.into_bytes())))
}

/// Decodes gist text (whitespace ignored) to package bytes, verifying header and checksum.
pub fn decode_gist_text(text: &str) -> Result<Vec<u8>> {
    let clean: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let bytes = STANDARD
        .decode(clean.as_bytes())
        .map_err(|e| CpmError::Invalid(format!("gist is not base64: {e}")))?;
    read_definition(&bytes)?;
    Ok(bytes)
}

/// A `.cpmmodel` whose data is `SKIN_TYPE`, a `PACKAGE_LINK` holding `link`, then `END`.
pub fn link_model(name: &str, desc: &str, skin_type: SkinType, link: &str) -> Vec<u8> {
    let mut skin = Writer::new();
    skin.u8(match skin_type {
        SkinType::Slim => 0,
        SkinType::Default => 1,
        SkinType::Unknown => 2,
    });
    let mut payload = Writer::new();
    payload.u8(link.len() as u8).bytes(link.as_bytes());
    let mut body = Writer::new();
    body.object_block(SKIN_TYPE, &skin)
        .object_block(PACKAGE_LINK, &payload)
        .object_block(END, &Writer::new());
    let data = checksummed(HEADER, &body.into_bytes());

    let mut d = Writer::new();
    d.bytes(&data);
    let mut w = Writer::new();
    w.utf(name).utf(desc).block(&d).varint(0).block(&Writer::new()); // reserved 0, empty icon
    checksummed(HEADER, &w.into_bytes())
}

/// `PasteClient.URL` and `URL_CF`: the paste site and its fallback.
const PASTE: &str = "https://paste.tom5454.com";
const PASTE_CF: &str = "https://cf-paste.tom5454.com";

/// The URLs a link (`loader:path`) downloads from, in the order tried (`ModelDefinitionLoader.
/// LOADERS`, each loader's `createURL`): only `p:` has a second one, the fallback
/// `PasteResourceLoader` turns to when the first does not connect. `local:` is always an error
/// (`Test in-game model`), as is an unknown loader.
pub fn link_urls(link: &str) -> Result<Vec<String>> {
    let unsupported = || CpmError::Invalid("unsupported link".into());
    let (loader, path) = link.split_once(':').ok_or_else(unsupported)?;
    if loader == "local" {
        return Err(CpmError::Invalid("a test in-game model (only CPM's editor loads it)".into()));
    }
    if path.is_empty() || path.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(unsupported());
    }
    Ok(match loader {
        "git" => vec![gist_raw_url(link).ok_or_else(unsupported)?],
        "gh" => vec![format!("https://raw.githubusercontent.com/{path}")],
        "p" => vec![format!("{PASTE}/raw/{path}"), format!("{PASTE_CF}/raw/{path}")],
        "pb" => vec![format!("https://pastebin.com/raw/{path}")],
        "ms" => vec![format!("https://cdn-cpmmodels.tom5454.com/content/{path}")],
        _ => return Err(unsupported()),
    })
}

/// Downloads `link`'s text through `fetch(url)`: each of [`link_urls`] in turn until one answers.
pub fn fetch_link(link: &str, fetch: &dyn Fn(&str) -> Result<Vec<u8>>) -> Result<Vec<u8>> {
    let mut last = None;
    for url in link_urls(link)? {
        match fetch(&url) {
            Ok(text) => return Ok(text),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| CpmError::Invalid("unsupported link".into())))
}

/// A link's downloaded text, base64-decoded (`ResourceEncoding.BASE64`). Whitespace is ignored,
/// as [`decode_gist_text`] does: a little more lenient than Java's basic decoder, which refuses a
/// text with a line break.
pub fn decode_link_text(text: &[u8]) -> Result<Vec<u8>> {
    let clean: Vec<u8> = text.iter().copied().filter(|c| !c.is_ascii_whitespace()).collect();
    STANDARD.decode(&clean).map_err(|e| CpmError::Invalid(format!("link is not base64: {e}")))
}

/// A link target's text as a loader serves it: the base64 of `target` (inverse of
/// [`decode_link_text`]).
pub fn encode_link_text(target: &[u8]) -> String {
    STANDARD.encode(target)
}

/// Loads a model, resolving every link part (a link model's package, a definition or skin link)
/// through `fetch(url)` ([`fetch_link`]: each link's URLs in turn).
pub fn load_resolved(bytes: &[u8], fetch: &dyn Fn(&str) -> Result<Vec<u8>>) -> Result<LoadedModel> {
    load_with_links(bytes, &|link| fetch_link(link, fetch))
}

/// Loads a model, resolving every link part through `text(link)`: the link's downloaded text
/// (base64). A linked package holding links is refused (`Pack`).
pub fn load_with_links(bytes: &[u8], text: &dyn Fn(&str) -> Result<Vec<u8>>) -> Result<LoadedModel> {
    let file = read_cpmmodel_resolving(bytes, &|link| decode_link_text(&text(link)?))?;
    LoadedModel::from_file(file)
}
