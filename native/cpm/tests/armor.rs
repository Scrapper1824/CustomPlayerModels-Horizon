// Worn armour and elytra in the mod's runtime: which texture an item draws, leather's dye, the
// UVs of boxes that sample a texture of their own, and the model's elytra sheet in the uploaded
// image. Each test runs on its own thread, so its own state.
mod common;

use std::path::{Path, PathBuf};
use std::time::Instant;

use cpm_core::{ArmorSlot, BoxSource, BoxTexture, Equipment, PoseInput, PosedBox, Sheet};
use horizon_mod_cpm as cpm;

fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("mixins/cpm/cpm-core/tests/samples").join(name)
}

fn layer(sprite: &str, tint: u32) -> cpm::WornLayer {
    cpm::WornLayer { sprite: sprite.into(), tint }
}

fn sprites(layers: &[cpm::WornLayer]) -> Vec<&str> {
    layers.iter().map(|l| l.sprite.as_str()).collect()
}

#[test]
fn armor_items_map_to_horizons_equipment_textures() {
    use ArmorSlot::*;
    let one = |item, slot| sprites(&cpm::armor_layers(item, slot, None)).join(",");
    assert_eq!(one("minecraft:iron_helmet", Helmet), "entity/equipment/humanoid/iron");
    assert_eq!(one("minecraft:iron_chestplate", Chestplate), "entity/equipment/humanoid/iron");
    assert_eq!(one("minecraft:iron_leggings", Leggings), "entity/equipment/humanoid_leggings/iron");
    assert_eq!(one("minecraft:iron_boots", Boots), "entity/equipment/humanoid/iron");
    assert_eq!(one("minecraft:golden_chestplate", Chestplate), "entity/equipment/humanoid/gold");
    assert_eq!(one("minecraft:golden_leggings", Leggings), "entity/equipment/humanoid_leggings/gold");
    assert_eq!(one("minecraft:turtle_helmet", Helmet), "entity/equipment/humanoid/turtle_scute");
    assert_eq!(one("minecraft:netherite_boots", Boots), "entity/equipment/humanoid/netherite");
    assert_eq!(one("minecraft:netherite_leggings", Leggings), "entity/equipment/humanoid_leggings/netherite");
    assert_eq!(one("minecraft:chainmail_helmet", Helmet), "entity/equipment/humanoid/chainmail");
    assert_eq!(
        one("minecraft:leather_leggings", Leggings),
        "entity/equipment/humanoid_leggings/leather,entity/equipment/humanoid_leggings/leather_overlay"
    );
    // Unknown items, and pieces in another slot, draw nothing.
    for (item, slot) in [
        ("minecraft:elytra", Chestplate),
        ("minecraft:carved_pumpkin", Helmet),
        ("minecraft:player_head", Helmet),
        ("gems:ruby_helmet", Helmet),
        ("minecraft:iron_helmet", Boots),
        ("minecraft:_helmet", Helmet),
        ("iron_helmet", Helmet),
    ] {
        assert!(cpm::armor_layers(item, slot, None).is_empty(), "{item}");
        assert_eq!(cpm::armor_asset(item, slot), None, "{item}");
    }
    // The elytra is the chest slot's wings, not an armour piece.
    assert!(cpm::is_elytra("minecraft:elytra"));
    assert!(!cpm::is_elytra("minecraft:iron_chestplate"));
    assert_eq!(cpm::ELYTRA_SPRITE, "entity/equipment/wings/elytra");
}

#[test]
fn leather_takes_the_dye_and_an_untinted_overlay() {
    let undyed = cpm::armor_layers("minecraft:leather_helmet", ArmorSlot::Helmet, None);
    assert_eq!(
        undyed,
        vec![
            layer("entity/equipment/humanoid/leather", 0xA0_6540),
            layer("entity/equipment/humanoid/leather_overlay", 0xFF_FFFF),
        ]
    );
    let dyed = cpm::armor_layers("minecraft:leather_boots", ArmorSlot::Boots, Some(0x3366_CC));
    assert_eq!(dyed[0], layer("entity/equipment/humanoid/leather", 0x33_66CC));
    assert_eq!(dyed[1].tint, 0xFF_FFFF);
    // Only leather is dyeable.
    let iron = cpm::armor_layers("minecraft:iron_boots", ArmorSlot::Boots, Some(0x3366_CC));
    assert_eq!(iron, vec![layer("entity/equipment/humanoid/iron", 0xFF_FFFF)]);
    assert_eq!(cpm::mul_rgb(0xFF_FFFF, 0x33_66CC), 0x33_66CC);
    assert_eq!(cpm::mul_rgb(0x80_8080, 0xFF_0000), 0x80_0000);
}

#[test]
fn worn_slots_with_textures_are_the_equipment() {
    let mut worn = cpm::Worn::default();
    assert_eq!(worn.equipment(true), Equipment::default());
    worn.armor[0] = cpm::armor_layers("minecraft:iron_helmet", ArmorSlot::Helmet, None);
    worn.armor[2] = cpm::armor_layers("minecraft:leather_leggings", ArmorSlot::Leggings, None);
    worn.elytra = true;
    let e = worn.equipment(false);
    assert!(e.head && e.legs && !e.chest && !e.feet);
    // An elytra with no texture of its own shows only with the model's sheet.
    assert!(!e.elytra);
    assert!(worn.equipment(true).elytra);
    worn.elytra_sprite = Some(cpm::ELYTRA_SPRITE.into());
    assert!(worn.equipment(false).elytra);
}

#[test]
fn non_skin_uvs_are_the_fraction_of_their_own_texture() {
    // 64x32 armour sheet: the helmet's front (8,8)-(16,16) is (1/8, 1/4)-(1/4, 1/2) of it.
    assert_eq!(cpm::own_uv([8.0, 8.0, 16.0, 16.0], [64, 32]), [2.0, 4.0, 4.0, 8.0]);
    // Mirrored faces keep their order.
    assert_eq!(cpm::own_uv([16.0, 0.0, 8.0, 32.0], [64, 32]), [4.0, 0.0, 2.0, 16.0]);
    assert_eq!(cpm::own_uv([32.0, 32.0, 64.0, 64.0], [128, 64]), [4.0, 8.0, 8.0, 16.0]);
    // An image at (68, 0) of the mod sprite, 128x64 real for 64x32 declared.
    assert_eq!(
        cpm::slot_uv([8.0, 8.0, 16.0, 16.0], [64, 32], [128, 64], [68, 0]),
        cpm::uv_to_sprite([84.0, 16.0, 100.0, 32.0])
    );
}

/// A box of the loaded model re-dressed as a box sampling `texture` (`texture_px` UV space).
fn redressed(texture: BoxTexture, px: [u32; 2]) -> PosedBox {
    let mut b = cpm::with_model(|m| m.pose(&PoseInput::default()))
        .unwrap()
        .into_iter()
        .find(|b| b.textured && b.faces.iter().flatten().count() == 6)
        .expect("a textured box");
    b.texture = texture;
    b.texture_px = px;
    b.tint = 0xFF_FFFF;
    b.glow = false;
    b
}

#[test]
fn armor_boxes_draw_each_worn_layer_on_its_own_sprite() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    cpm::select(Some(sample("ReggieTheRat.cpmmodel")));
    cpm::ensure_loaded();
    let helmet = redressed(BoxTexture::Armor(ArmorSlot::Helmet), [64, 32]);
    let legs = redressed(BoxTexture::Armor(ArmorSlot::Leggings), [64, 32]);
    let mut worn = cpm::Worn::default();
    worn.armor[0] = cpm::armor_layers("minecraft:iron_helmet", ArmorSlot::Helmet, None);
    worn.armor[2] = cpm::armor_layers("minecraft:leather_leggings", ArmorSlot::Leggings, Some(0x8040_20));
    let q = cpm::with_model(|m| cpm::box_quads(m, &[helmet.clone(), legs.clone()], &worn)).unwrap();
    assert_eq!(
        q.sprites,
        vec![
            cpm::SPRITE,
            "entity/equipment/humanoid/iron",
            "entity/equipment/humanoid_leggings/leather",
            "entity/equipment/humanoid_leggings/leather_overlay",
        ]
    );
    // Helmet: 6 faces on the iron sprite; leggings: 6 tinted, then 6 overlay faces.
    let on = |i: usize| q.quads.iter().filter(|x| x.sprite == i).collect::<Vec<_>>();
    assert_eq!((on(0).len(), on(1).len(), on(2).len(), on(3).len()), (0, 6, 6, 6));
    assert!(on(1).iter().all(|x| x.tint == 0xFF_FFFF));
    assert!(on(2).iter().all(|x| x.tint == 0x80_4020));
    assert!(on(3).iter().all(|x| x.tint == 0xFF_FFFF));
    let faces: Vec<_> = helmet.faces.iter().flatten().collect();
    for (x, f) in on(1).iter().zip(&faces) {
        assert_eq!(x.uv, cpm::own_uv(f.uv, [64, 32]));
        assert!(x.uv.iter().all(|v| (0.0..=16.0).contains(v)), "{:?}", x.uv);
    }
    // The overlay is the same faces again.
    for (a, b) in on(2).iter().zip(on(3)) {
        assert_eq!((a.corners, a.uv), (b.corners, b.uv));
    }
    // A slot with nothing worn (or no texture) draws nothing.
    let none = cpm::with_model(|m| cpm::box_quads(m, &[helmet], &cpm::Worn::default())).unwrap();
    assert!(none.quads.is_empty());
}

#[test]
fn untextured_armor_cubes_use_the_white_patch_once() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    cpm::select(Some(sample("ReggieTheRat.cpmmodel")));
    cpm::ensure_loaded();
    let mut b = redressed(BoxTexture::Armor(ArmorSlot::Leggings), [64, 32]);
    b.textured = false;
    b.tint = 0x12_3456;
    let mut worn = cpm::Worn::default();
    worn.armor[2] = cpm::armor_layers("minecraft:leather_leggings", ArmorSlot::Leggings, None);
    let q = cpm::with_model(|m| cpm::box_quads(m, &[b], &worn)).unwrap();
    let png_w = cpm::with_model(|m| m.png_px[0]).unwrap();
    assert_eq!(q.quads.len(), 6);
    assert!(q.quads.iter().all(|x| x.sprite == 0 && x.uv == cpm::white_uv(png_w) && x.tint == 0x12_3456));
}

fn red_sheet(w: u32, h: u32, declared: [u32; 2]) -> Sheet {
    Sheet { rgba: [255, 0, 0, 255].repeat((w * h) as usize), texture_px: declared, png_px: [w, h] }
}

#[test]
fn composite_places_the_elytra_sheet_after_the_white_patch() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    assert_eq!(
        cpm::composite([64, 64], None, None),
        cpm::Composite { size: [68, 64], white: [64, 0], elytra: None, cape: None }
    );
    assert_eq!(
        cpm::composite([64, 64], Some([64, 32]), None),
        cpm::Composite { size: [132, 64], white: [64, 0], elytra: Some([68, 0]), cape: None }
    );
    // A taller sheet sets the height.
    assert_eq!(cpm::composite([64, 32], Some([128, 64]), None).size, [196, 64]);
    assert_eq!(cpm::composite([2, 2], None, None).size, [6, 4]);
    // The cape sheet follows the elytra sheet, or the white patch without one.
    assert_eq!(
        cpm::composite([64, 64], Some([64, 32]), Some([128, 96])),
        cpm::Composite { size: [260, 96], white: [64, 0], elytra: Some([68, 0]), cape: Some([132, 0]) }
    );
    assert_eq!(cpm::composite([64, 64], None, Some([64, 32])).cape, Some([68, 0]));

    cpm::select(Some(sample("ReggieTheRat.cpmmodel")));
    cpm::ensure_loaded();
    let [pw, ph] = cpm::with_model(|m| m.png_px).unwrap();
    cpm::with_model(|m| m.elytra = Some(red_sheet(64, 32, [64, 32]))).unwrap();
    let (w, h, rgba) = cpm::texture_upload(Instant::now()).expect("upload");
    assert_eq!((w, h), (pw + 4 + 64, ph.max(32)));
    let px = |x: u32, y: u32| &rgba[((y * w + x) * 4) as usize..][..4];
    assert_eq!(px(pw + 1, 1), [255; 4], "white patch");
    assert_eq!(px(pw + 4, 0), [255, 0, 0, 255], "sheet's first pixel");
    assert_eq!(px(w - 1, 31), [255, 0, 0, 255], "sheet's last pixel");
    let skin = cpm::with_model(|m| m.texture_rgba[..4].to_vec()).unwrap();
    assert_eq!(px(0, 0), &skin[..]);

    // Elytra boxes sample the sheet's place in the mod sprite, even with a worn elytra.
    let wing = redressed(BoxTexture::Elytra, [64, 32]);
    let worn = cpm::Worn { elytra: true, elytra_sprite: Some(cpm::ELYTRA_SPRITE.into()), ..Default::default() };
    let q = cpm::with_model(|m| cpm::box_quads(m, &[wing.clone()], &worn)).unwrap();
    assert_eq!(q.sprites, vec![cpm::SPRITE]);
    for (x, f) in q.quads.iter().zip(wing.faces.iter().flatten()) {
        assert_eq!(x.sprite, 0);
        assert_eq!(x.uv, cpm::slot_uv(f.uv, [64, 32], [64, 32], [pw + 4, 0]));
        let max = w as f32 * 16.0 / 1024.0;
        let lo = (pw + 4) as f32 * 16.0 / 1024.0;
        assert!(x.uv.iter().step_by(2).all(|u| (lo..=max).contains(u)), "{:?}", x.uv);
    }
}

#[test]
fn elytra_without_a_sheet_uses_the_worn_texture_or_nothing() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    cpm::select(Some(sample("ReggieTheRat.cpmmodel")));
    cpm::ensure_loaded();
    cpm::with_model(|m| m.elytra = None).unwrap();
    let wing = redressed(BoxTexture::Elytra, [64, 32]);
    let worn = cpm::Worn { elytra: true, elytra_sprite: Some(cpm::ELYTRA_SPRITE.into()), ..Default::default() };
    let q = cpm::with_model(|m| cpm::box_quads(m, &[wing.clone()], &worn)).unwrap();
    assert_eq!(q.sprites, vec![cpm::SPRITE, cpm::ELYTRA_SPRITE]);
    assert!(q.quads.iter().all(|x| x.sprite == 1));
    let none = cpm::Worn { elytra: true, ..Default::default() };
    assert!(cpm::with_model(|m| cpm::box_quads(m, &[wing], &none)).unwrap().quads.is_empty());
}

#[test]
fn elytra_sheet_too_big_is_refused() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    cpm::select(Some(sample("ReggieTheRat.cpmmodel")));
    cpm::ensure_loaded();
    cpm::with_model(|m| m.elytra = Some(red_sheet(1000, 4, [64, 32]))).unwrap();
    assert!(cpm::texture_upload(Instant::now()).is_none());
    let e = cpm::last_error().unwrap();
    assert!(e.contains("elytra sheet") && e.contains("too big"), "{e}");
}

#[test]
fn quads_dress_the_model_in_what_is_worn() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    cpm::select(Some(sample("ReggieTheRat.cpmmodel")));
    cpm::ensure_loaded();
    cpm::with_model(|m| m.elytra = None).unwrap();
    cpm::texture_upload(Instant::now()).unwrap();
    let input = PoseInput { on_ground: true, ..PoseInput::default() };
    let bare = cpm::quads(&input, &cpm::Worn::default()).unwrap();
    assert_eq!(bare.sprites, vec![cpm::SPRITE]);
    // The input's own equipment is ignored: what is worn decides.
    let claims = PoseInput { equipment: Equipment { head: true, elytra: true, ..Default::default() }, ..input };
    assert_eq!(cpm::quads(&claims, &cpm::Worn::default()).unwrap(), bare);

    let mut worn = cpm::Worn::default();
    worn.armor[0] = cpm::armor_layers("minecraft:diamond_helmet", ArmorSlot::Helmet, None);
    worn.elytra = true;
    worn.elytra_sprite = Some(cpm::ELYTRA_SPRITE.into());
    let dressed = cpm::quads(&input, &worn).unwrap();
    // Exactly the model's boxes for that equipment, the helmet's on its sprite.
    let equipment = worn.equipment(false);
    let boxes = cpm::with_model(|m| m.pose(&PoseInput { equipment, ..input })).unwrap();
    let want = cpm::with_model(|m| cpm::box_quads(m, &boxes, &worn)).unwrap();
    assert_eq!((&dressed.sprites, &dressed.quads), (&want.sprites, &want.quads));
    // With the items in both hands and on the head (Reggie has no item parts: its arms and head).
    assert_eq!(dressed.items.len(), 3);
    let helmet = boxes.iter().filter(|b| b.texture == BoxTexture::Armor(ArmorSlot::Helmet)).count();
    let wings = boxes.iter().filter(|b| b.texture == BoxTexture::Elytra).count();
    assert!(wings > 0, "the elytra shows");
    let on = |name: &str| {
        let i = dressed.sprites.iter().position(|s| s == name);
        i.map_or(0, |i| dressed.quads.iter().filter(|x| x.sprite == i).count())
    };
    if helmet > 0 {
        assert!(on("entity/equipment/humanoid/diamond") > 0);
    }
    assert!(on(cpm::ELYTRA_SPRITE) > 0);
    // Vanilla parts of a worn slot, if any, come from RootModelValues on the 64x32 sheet.
    for b in boxes.iter().filter(|b| matches!(b.source, BoxSource::RootVanilla(_))) {
        assert_eq!(b.texture_px, [64, 32]);
    }
}

#[test]
fn cape_texture_prefers_model_sheet() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    cpm::select(Some(sample("ReggieTheRat.cpmmodel")));
    cpm::ensure_loaded();
    let [pw, _] = cpm::with_model(|m| m.png_px).unwrap();
    cpm::with_model(|m| {
        m.elytra = None;
        m.cape = Some(red_sheet(128, 64, [64, 32]));
    })
    .unwrap();
    let (w, _, rgba) = cpm::texture_upload(Instant::now()).expect("upload");
    assert_eq!(w, pw + 4 + 128);
    assert_eq!(&rgba[((pw + 4) * 4) as usize..][..4], [255, 0, 0, 255], "cape sheet after the white patch");
    let cape = redressed(BoxTexture::Cape, [64, 32]);
    // The model's sheet wins over the player's cape.
    let worn = cpm::Worn { cape_sprite: Some("skins/cape".into()), ..Default::default() };
    let q = cpm::with_model(|m| cpm::box_quads(m, &[cape.clone()], &worn)).unwrap();
    assert_eq!(q.sprites, vec![cpm::SPRITE]);
    assert!(!q.quads.is_empty());
    for (x, f) in q.quads.iter().zip(cape.faces.iter().flatten()) {
        assert_eq!(x.sprite, 0);
        assert_eq!(x.uv, cpm::slot_uv(f.uv, [64, 32], [128, 64], [pw + 4, 0]));
    }
    // Without a sheet: the player's cape.
    cpm::with_model(|m| m.cape = None).unwrap();
    let q = cpm::with_model(|m| cpm::box_quads(m, &[cape.clone()], &worn)).unwrap();
    assert_eq!(q.sprites, vec![cpm::SPRITE, "skins/cape"]);
    assert!(!q.quads.is_empty() && q.quads.iter().all(|x| x.sprite == 1));
    // The vanilla cape (no cape root) takes the player's cape even when the model has a sheet.
    cpm::with_model(|m| m.cape = Some(red_sheet(64, 32, [64, 32]))).unwrap();
    let vanilla = redressed(BoxTexture::PlayerCape, [64, 32]);
    let q = cpm::with_model(|m| cpm::box_quads(m, &[vanilla], &worn)).unwrap();
    assert_eq!(q.sprites, vec![cpm::SPRITE, "skins/cape"]);
    assert!(!q.quads.is_empty() && q.quads.iter().all(|x| x.sprite == 1));
}

#[test]
fn no_cape_texture_draws_nothing() {
    if common::missing(&["ReggieTheRat.cpmmodel"]) {
        return;
    }
    cpm::select(Some(sample("ReggieTheRat.cpmmodel")));
    cpm::ensure_loaded();
    cpm::with_model(|m| m.cape = None).unwrap();
    let none = cpm::Worn::default();
    for t in [BoxTexture::Cape, BoxTexture::PlayerCape] {
        let b = redressed(t, [64, 32]);
        assert!(cpm::with_model(|m| cpm::box_quads(m, &[b], &none)).unwrap().quads.is_empty(), "{t:?}");
    }
    // Drawn through the whole model: a cape layer with no texture adds nothing.
    cpm::texture_upload(Instant::now()).unwrap();
    let input = PoseInput { on_ground: true, ..PoseInput::default() };
    let caped = PoseInput { cape: Some(cpm_core::CapeState::default()), ..input };
    assert_eq!(cpm::quads(&caped, &none).unwrap(), cpm::quads(&input, &none).unwrap());
    // With the player's cape, the vanilla cape is drawn in it.
    let worn = cpm::Worn { cape_sprite: Some("skins/cape".into()), ..Default::default() };
    let q = cpm::quads(&caped, &worn).unwrap();
    assert_eq!(q.sprites, vec![cpm::SPRITE, "skins/cape"]);
    assert_eq!(q.quads.iter().filter(|x| x.sprite == 1).count(), 6);
    // Not under an elytra's wings.
    let wings = cpm::Worn { chest_has_wings: true, ..worn.clone() };
    assert!(cpm::quads(&caped, &wings).unwrap().quads.iter().all(|x| x.sprite == 0));
}
