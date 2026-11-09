// A model stored in a skin's free space (`SkinDataInputStream`, `ModelDefinitionLoader.loadModel(
// Image, Player)`): written with CPM's encoder rules (`SkinDataOutputStream`), read back.

use cpm_core::format::SkinType;
use cpm_core::skin::{self, Image, SkinDataInputStream, SkinDataOutputStream};
use cpm_core::write::{Writer, checksummed};

const HEADER: u8 = 0x53;
const END: u8 = 0;
const SKIN_TYPE: u8 = 11;
/// A part type past `ModelPartType.VALUES`: read and skipped by its block length.
const FILLER: u8 = 99;

/// Model data (`0x53`, the parts, the checksum): the skin type, a filler part of `filler` bytes,
/// then `END`.
fn data(skin_type: u8, filler: usize) -> Vec<u8> {
    let mut body = Writer::new();
    let mut t = Writer::new();
    t.u8(skin_type);
    body.object_block(SKIN_TYPE, &t);
    let mut f = Writer::new();
    f.bytes(&(0..filler).map(|i| (i * 7 + 3) as u8).collect::<Vec<_>>());
    body.object_block(FILLER, &f).object_block(END, &Writer::new());
    checksummed(HEADER, &body.into_bytes())
}

/// A 64x64 skin with opaque grey pixels everywhere, `data` written into it on `channel`.
fn skin_with(data: &[u8], channel: i32) -> Image {
    let mut img = Image::new(64, 64);
    for p in img.argb.iter_mut() {
        *p = 0xFF80_8080;
    }
    let template = skin::template().unwrap();
    let mut out = SkinDataOutputStream::new(&mut img, template, channel);
    for &b in data {
        out.write(b).unwrap();
    }
    img
}

#[test]
fn skin_data_round_trip() {
    for (ty, channel, skin_type) in [(1u8, 1, SkinType::Default), (0, 0, SkinType::Slim)] {
        let bytes = data(ty, 600);
        let img = skin_with(&bytes, channel);
        // Byte by byte, as the stream reads it.
        let template = skin::template().unwrap();
        let mut sin = SkinDataInputStream::new(&img, template, channel);
        for (i, &b) in bytes.iter().enumerate() {
            assert_eq!(sin.read().unwrap(), Some(b), "byte {i}");
        }
        // Through a PNG (the raw skin file), the whole definition.
        let png = img.encode_png();
        let back = Image::decode(&png).unwrap();
        assert_eq!(back, img);
        assert_eq!(skin::read_skin_data(&back, skin_type).unwrap(), Some(bytes.clone()));
        // As a .cpmmodel the loader reads.
        let file = skin::skin_model_file(&png, skin_type).unwrap().expect("a model");
        let m = cpm_core::format::read_cpmmodel(&file).unwrap();
        assert_eq!(m.data.skin_type, if ty == 0 { SkinType::Slim } else { SkinType::Default });
    }
}

#[test]
fn three_or_four_bytes_per_pixel() {
    // A template pixel with blue 0xff holds 3 bytes (blue, green, red) and stays opaque; any
    // other holds 4 (alpha too).
    let template = skin::template().unwrap();
    let mut img = Image::new(64, 64);
    let bytes: Vec<u8> = (1..=40).collect();
    let mut out = SkinDataOutputStream::new(&mut img, template, 1);
    for &b in &bytes {
        out.write(b).unwrap();
    }
    // The first template pixel (0, 0) is white: 3 bytes, opaque.
    assert_eq!(template.get_rgb(0, 0).unwrap(), 0xFFFF_FFFF);
    assert_eq!(img.get_rgb(0, 0).unwrap(), 0xFF03_0201);
    let mut sin = SkinDataInputStream::new(&img, template, 1);
    let read: Vec<u8> = (0..40).map(|_| sin.read().unwrap().unwrap()).collect();
    assert_eq!(read, bytes);
}

#[test]
fn skin_channel_by_skin_type() {
    assert_eq!(skin::channel(SkinType::Slim), 0);
    assert_eq!(skin::channel(SkinType::Default), 1);
    assert_eq!(skin::channel(SkinType::Unknown), -1);
    // Long enough to pass the slim-only pixels (red only in the template), which the default
    // channel skips.
    let slim = data(0, 2600);
    let img = skin_with(&slim, 0);
    assert_eq!(skin::read_skin_data(&img, SkinType::Slim).unwrap(), Some(slim.clone()));
    // A wide player whose skin holds a slim model: the shared pixels give the header and the skin
    // type, then the reader turns to the slim channel (the mismatch handling).
    assert_eq!(skin::read_skin_data(&img, SkinType::Default).unwrap(), Some(slim.clone()));
    // Data written for the wide channel has no slim-only pixels: a wide model read by a slim
    // player turns to the wide channel the same way.
    let wide = data(1, 2600);
    let img = skin_with(&wide, 1);
    assert_eq!(skin::read_skin_data(&img, SkinType::Slim).unwrap(), Some(wide.clone()));
    // An unknown skin type in the data keeps the reader's channel: a slim player reading wide
    // data marked unknown reads the slim-only pixels too, and the checksum fails.
    let unknown = data(2, 2600);
    let img = skin_with(&unknown, 1);
    assert_eq!(skin::read_skin_data(&img, SkinType::Default).unwrap(), Some(unknown.clone()));
    assert_ne!(skin::read_skin_data(&img, SkinType::Slim).ok(), Some(Some(unknown)));
}

#[test]
fn skin_without_data_is_none() {
    // No header in the first free pixel: no model, no error.
    let blank = Image::new(64, 64);
    assert_eq!(skin::read_skin_data(&blank, SkinType::Default).unwrap(), None);
    let mut grey = Image::new(64, 64);
    grey.argb.fill(0xFF80_8080);
    assert_eq!(skin::read_skin_data(&grey, SkinType::Slim).unwrap(), None);
    assert_eq!(skin::skin_model_file(&grey.encode_png(), SkinType::Slim).unwrap(), None);
    // A skin with no free pixel at all (too small for the template's first one).
    let tiny = Image::new(0, 0);
    assert_eq!(skin::read_skin_data(&tiny, SkinType::Default).unwrap(), None);
    // Not a PNG: an error.
    assert!(skin::skin_model_file(b"not a png", SkinType::Default).is_err());
}

#[test]
fn corrupt_skin_data_is_an_error() {
    // The header, then data that ends early (every free byte is 0x53 0xff...): an error, as the
    // Java loader's `ModelDefinition` with an error.
    let mut bytes = data(1, 100);
    let n = bytes.len();
    bytes[n - 1] ^= 0xFF; // bad checksum
    let img = skin_with(&bytes, 1);
    let file = skin::skin_model_file(&img.encode_png(), SkinType::Default).unwrap().expect("a model");
    assert!(matches!(cpm_core::format::read_cpmmodel(&file), Err(cpm_core::CpmError::Checksum { .. })));
    // The free space ends before END.
    let mut img = Image::new(64, 64);
    let template = skin::template().unwrap();
    let mut out = SkinDataOutputStream::new(&mut img, template, 1);
    for b in [HEADER, FILLER, 0xFF, 0xFF, 0x03] {
        out.write(b).unwrap();
    }
    assert!(skin::read_skin_data(&img, SkinType::Default).is_err());
}

#[test]
fn skin_texture_when_the_model_has_none() {
    // A link-free model with one textured cube and no texture of its own draws with the skin.
    let mut body = Writer::new();
    let mut t = Writer::new();
    t.u8(1);
    body.object_block(SKIN_TYPE, &t).object_block(END, &Writer::new());
    let bytes = checksummed(HEADER, &body.into_bytes());
    let img = skin_with(&bytes, 1);
    let png = img.encode_png();
    let file = skin::skin_model_file(&png, SkinType::Default).unwrap().unwrap();
    let mut model = cpm_core::load(&file).unwrap();
    assert!(model.texture_rgba.is_empty());
    let rgba: Vec<u8> = img.argb.iter().flat_map(|p| {
        let [a, r, g, b] = p.to_be_bytes();
        [r, g, b, a]
    }).collect();
    assert!(skin::use_skin_texture(&mut model, [64, 64], rgba.clone()));
    assert_eq!((model.png_px, model.texture_px, model.model.texture_size), ([64, 64], [64, 64], [64, 64]));
    assert_eq!(model.texture_rgba, rgba);
    // A model with its own texture keeps it.
    assert!(!skin::use_skin_texture(&mut model, [64, 64], vec![0; 64 * 64 * 4]));
    assert_eq!(model.texture_rgba, rgba);
}
