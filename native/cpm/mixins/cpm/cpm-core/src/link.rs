//! GitHub-gist links and link models.
//!
//! A *link model* is a `.cpmmodel` whose data is `SKIN_TYPE`, a `PACKAGE_LINK` and `END`
//! (`Exporter.writeOut`). The linked gist holds the base64 of the *package*: `0x53`, the
//! definition's parts (`PartCollection.writePackage`: every part but the skin type, then
//! `END`) and the 2-byte checksum.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::format::{HEADER, ModelFile, SkinType, read_cpmmodel, read_definition};
use crate::io::{CpmError, Reader, Result};
use crate::mesh::LoadedModel;
use crate::write::{Writer, checksummed};

const END: u8 = 0;
const SKIN_TYPE: u8 = 11;
const PACKAGE_LINK: u8 = 20;
/// `DEFINITION_LINK`, `SKIN_LINK` and `PACKAGE_LINK`: refused inside a package (`Pack`).
const LINK_PARTS: [u8; 3] = [4, 6, 20];

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

/// Loads a model, resolving a link model through `fetch(raw_url)` (gist text bytes).
pub fn load_resolved(bytes: &[u8], fetch: &dyn Fn(&str) -> Result<Vec<u8>>) -> Result<LoadedModel> {
    let file = read_cpmmodel(bytes)?;
    let Some(link) = file.data.link.clone() else {
        return LoadedModel::from_file(file);
    };
    let url = gist_raw_url(&link).ok_or_else(|| CpmError::Invalid(format!("unsupported link: {link}")))?;
    let text = String::from_utf8(fetch(&url)?).map_err(|_| CpmError::Invalid("gist is not text".into()))?;
    let pkg = decode_gist_text(&text)?;
    check_no_links(&pkg)?;
    let mut data = read_definition(&pkg)?;
    data.skin_type = file.data.skin_type; // the outer SKIN_TYPE wins
    data.link = None;
    LoadedModel::from_file(ModelFile { name: file.name, desc: file.desc, data, icon_png: file.icon_png })
}

/// `Pack` rule: a package must not contain link parts.
fn check_no_links(pkg: &[u8]) -> Result<()> {
    let mut r = Reader::new(pkg);
    r.u8()?;
    loop {
        let ty = r.u8()?;
        r.block()?;
        if LINK_PARTS.contains(&ty) {
            return Err(CpmError::Invalid("invalid tag in package".into()));
        }
        if ty == END {
            return Ok(());
        }
    }
}
