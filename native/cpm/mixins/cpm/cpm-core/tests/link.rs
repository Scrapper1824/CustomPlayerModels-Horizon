mod common;

use cpm_core::format::{read_cpmmodel, read_definition, SkinType};
use cpm_core::io::CpmError;
use cpm_core::link::*;
use cpm_core::write::{checksummed, Writer};
use cpm_core::load;

fn reggie() -> Vec<u8> {
    std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/samples/ReggieTheRat.cpmmodel")).unwrap()
}

fn b64(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(b)
}

/// Reggie's data block without its SKIN_TYPE part (what `writePackage` emits), re-checksummed.
fn reggie_package() -> Vec<u8> {
    let mut body = Writer::new();
    let f = read_cpmmodel(&reggie()).unwrap();
    assert_eq!(f.data.cubes.len(), 26);
    // The sample is [SKIN_TYPE(1), DEFINITION(4146 bytes), END]; locate the DEFINITION payload.
    let full = reggie();
    let data = data_block(&full);
    let mut i = 1;
    loop {
        let ty = data[i];
        let mut j = i + 1;
        let (mut len, mut s) = (0usize, 0);
        loop {
            let b = data[j];
            j += 1;
            len |= ((b & 0x7F) as usize) << s;
            s += 7;
            if b & 0x80 == 0 {
                break;
            }
        }
        if ty != 11 {
            body.bytes(&data[i..j + len]);
        }
        i = j + len;
        if ty == 0 {
            break;
        }
    }
    checksummed(0x53, &body.into_bytes())
}

fn data_block(file: &[u8]) -> Vec<u8> {
    let mut r = cpm_core::io::Reader::new(file);
    r.u8().unwrap();
    r.utf().unwrap();
    r.utf().unwrap();
    let mut b = r.block().unwrap();
    let n = b.remaining();
    b.bytes(n).unwrap().to_vec()
}

#[test]
fn gist_url_variants() {
    let want = Some("git:alice/abc123".to_string());
    for u in [
        "https://gist.github.com/alice/abc123",
        "https://gist.github.com/alice/abc123/",
        "https://gist.github.com/alice/abc123#file-model-txt",
        "  https://gist.github.com/alice/abc123/raw  ",
        "https://gist.githubusercontent.com/alice/abc123/raw",
    ] {
        assert_eq!(parse_gist_url(u), want, "{u}");
    }
    for u in ["https://github.com/alice/abc123", "gist.github.com/alice", "https://gist.github.com/alice", ""] {
        assert_eq!(parse_gist_url(u), None, "{u}");
    }
}

#[test]
fn gist_raw_url_only_git() {
    assert_eq!(
        gist_raw_url("git:alice/abc").as_deref(),
        Some("https://gist.githubusercontent.com/alice/abc/raw")
    );
    assert_eq!(gist_raw_url("http:alice/abc"), None);
    assert_eq!(gist_raw_url("git:alice"), None);
}

#[test]
fn gist_text_roundtrip() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let text = gist_text(&reggie()).unwrap();
    let pkg = decode_gist_text(&text).unwrap();
    assert_eq!(pkg, reggie_package());
    assert_eq!(read_definition(&pkg).unwrap().cubes.len(), 26);
    // Whitespace/newlines (e.g. a wrapped or trailing-newline gist) are tolerated.
    let wrapped: String = text.as_bytes().chunks(60).map(|c| format!("{}\r\n", std::str::from_utf8(c).unwrap())).collect();
    assert_eq!(decode_gist_text(&wrapped).unwrap(), pkg);
}

#[test]
fn link_model_decodes() {
    let bytes = link_model("R", "d", SkinType::Default, "git:alice/abc");
    let f = read_cpmmodel(&bytes).unwrap();
    assert_eq!(f.name, "R");
    assert_eq!(f.desc, "d");
    assert_eq!(f.data.link.as_deref(), Some("git:alice/abc"));
    assert_eq!(f.data.skin_type, SkinType::Default);
    assert!(f.icon_png.is_empty());
    match load(&bytes) {
        Err(CpmError::NeedsLink(l)) => assert_eq!(l, "git:alice/abc"),
        other => panic!("{:?}", other.map(|m| m.name)),
    }
}

#[test]
fn load_resolved_with_fake_fetch() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let text = gist_text(&reggie()).unwrap();
    let bytes = link_model("R", "d", SkinType::Slim, "git:alice/abc");
    let m = load_resolved(&bytes, &|url| {
        assert_eq!(url, "https://gist.githubusercontent.com/alice/abc/raw");
        Ok(text.clone().into_bytes())
    })
    .unwrap();
    assert_eq!(m.model.cubes.len(), 26);
    assert_eq!(m.texture_px, [75, 64]);
    assert_eq!(m.name, "R");
}

#[test]
fn load_resolved_passes_plain_models_through() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let m = load_resolved(&reggie(), &|_| panic!("no fetch expected")).unwrap();
    assert_eq!(m.model.cubes.len(), 26);
}

#[test]
fn load_resolved_rejects_nested_link() {
    // A package that itself holds a PACKAGE_LINK part.
    let mut link = Writer::new();
    link.u8(5).bytes(b"git:a");
    let mut body = Writer::new();
    body.object_block(20, &link).object_block(0, &Writer::new());
    let text = b64(&checksummed(0x53, &body.into_bytes()));
    let bytes = link_model("R", "d", SkinType::Default, "git:alice/abc");
    assert!(load_resolved(&bytes, &|_| Ok(text.clone().into_bytes())).is_err());
}

#[test]
fn bad_gist_content_is_error() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    assert!(decode_gist_text("<!DOCTYPE html><html>nope</html>").is_err());
    let good = gist_text(&reggie()).unwrap();
    assert!(decode_gist_text(&good[..good.len() / 2]).is_err());
    assert!(decode_gist_text(&good[..good.len() - 8]).is_err());
    let mut pkg = reggie_package();
    pkg[0] = 0x54;
    assert!(decode_gist_text(&b64(&pkg)).is_err());
    let mut pkg = reggie_package();
    let n = pkg.len();
    pkg[n - 1] ^= 1;
    assert!(decode_gist_text(&b64(&pkg)).is_err());
    // And load_resolved surfaces it as an error, not a panic.
    let bytes = link_model("R", "d", SkinType::Default, "git:alice/abc");
    assert!(load_resolved(&bytes, &|_| Ok(b"<html>".to_vec())).is_err());
    assert!(load_resolved(&bytes, &|_| Err(CpmError::Invalid("offline".into()))).is_err());
}

/// A `.cpmmodel` as CPM's exporter writes a model too big for its data block
/// (`ModelFile.load`): the data block holds a PACKAGE_LINK to `link`, and the
/// overflow block holds the package itself, followed by the link (`Link.write`).
fn overflow_model(link: &str, package: &[u8]) -> Vec<u8> {
    let link_file = link_model("P", "d", SkinType::Slim, link);
    let data = data_block(&link_file);
    let mut w = Writer::new();
    let mut ovf = Writer::new();
    ovf.bytes(package);
    let mut d = Writer::new();
    d.bytes(&data);
    w.utf("P").utf("d").block(&d).block(&ovf);
    w.u8(link.len() as u8).bytes(link.as_bytes());
    w.block(&Writer::new()); // empty icon
    checksummed(0x53, &w.into_bytes())
}

/// A package holding an empty DEFINITION (no cubes), then END.
fn empty_package() -> Vec<u8> {
    let mut def = Writer::new();
    def.varint(0).object_block(0, &Writer::new());
    let mut body = Writer::new();
    body.object_block(3 /* DEFINITION */, &def).object_block(0, &Writer::new());
    checksummed(0x53, &body.into_bytes())
}

#[test]
fn overflow_block_is_read_and_resolves_the_local_link() {
    let bytes = overflow_model("p:bfae20505d5c4232b4e16f65aea7ac4b", &empty_package());
    let f = read_cpmmodel(&bytes).unwrap();
    assert_eq!(f.name, "P");
    assert_eq!(f.data.link, None, "the local link resolves to the overflow package");
    assert_eq!(f.data.skin_type, SkinType::Slim, "the outer SKIN_TYPE wins");
}

#[test]
fn overflow_model_loads_without_fetching() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    let bytes = overflow_model("p:abc", &reggie_package());
    let m = load_resolved(&bytes, &|_| panic!("no fetch expected")).unwrap();
    assert_eq!(m.model.cubes.len(), 26);
    let m = load(&bytes).unwrap();
    assert_eq!(m.model.cubes.len(), 26);
}

/// A `.cpmmodel` like CPM's own big-model export: the data block holds SKIN_TYPE and a
/// DEFINITION_LINK; the overflow holds `0x53`, a DEFINITION payload and its checksum.
#[test]
fn overflow_definition_link_is_read_in_place() {
    let link = "p:bfae20505d5c4232b4e16f65aea7ac4b";
    let mut lp = Writer::new();
    lp.u8(link.len() as u8).bytes(link.as_bytes());
    let mut skin = Writer::new();
    skin.u8(0);
    let mut body = Writer::new();
    body.object_block(11, &skin).object_block(4, &lp).object_block(0, &Writer::new());
    let data = checksummed(0x53, &body.into_bytes());
    let mut def = Writer::new();
    def.varint(0).object_block(0, &Writer::new());
    let target = checksummed(0x53, &def.into_bytes());

    let mut d = Writer::new();
    d.bytes(&data);
    let mut ovf = Writer::new();
    ovf.bytes(&target);
    let mut w = Writer::new();
    w.utf("P").utf("d").block(&d).block(&ovf);
    w.u8(link.len() as u8).bytes(link.as_bytes());
    w.block(&Writer::new());
    let f = read_cpmmodel(&checksummed(0x53, &w.into_bytes())).unwrap();
    assert_eq!(f.data.link, None);
    assert_eq!(f.data.skin_type, SkinType::Slim);
}
