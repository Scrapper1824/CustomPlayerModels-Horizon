// The gist popup: CPM's own mod screen, opened when a converted model is too big to send directly.
// Added to the game as `game::cpm_gist`; its state and logic are `cpm_mod::GistPopup`, so this
// only opens and closes the screen, draws it and turns the screen hooks' input into calls. The
// hooks reach it through `game::cpm_screen`, only while the popup is the CPM screen shown.
use crate::game::Game;
use crate::game::cpm_mod as cpm;
use horizon_client::mod_api::{HudBatch, NativeScreenView};

/// Once a frame: opens the screen for a due popup (when the game lets it: over the world with no
/// other screen up, so it waits out the settings page and the gesture menu), drops the popup
/// once its screen closed (Escape), and runs what the input hooks asked for (clipboard, closing).
pub(super) fn frame(game: &mut Game<'_>) {
    if !cpm::gist_popup_shown() {
        if let Some((model, size)) = cpm::pending_gist_popup()
            && cpm::shown_screen().is_none()
            && game.native_mod_screen_open(cpm::ID)
        {
            cpm::take_gist_popup();
            cpm::show_gist_popup(cpm::GistPopup::new(model, size));
        }
        return;
    }
    if game.native_mod_screen() != Some(cpm::ID) {
        cpm::hide_gist_popup();
        return;
    }
    for request in cpm::take_gist_requests() {
        match request {
            cpm::GistRequest::Copy => copy(game),
            cpm::GistRequest::Paste => {
                use horizon_client::clipboard::Clipboard;
                let text = game.clipboard.get();
                cpm::with_gist_popup(|p| p.paste(&text));
            }
            cpm::GistRequest::Close => {
                game.native_mod_screen_close(cpm::ID);
                cpm::hide_gist_popup();
                return;
            }
        }
    }
}

/// `Copy text`: the gist text file onto the game's clipboard, read back to check it got there.
fn copy(game: &mut Game<'_>) {
    use horizon_client::clipboard::Clipboard;
    let Some(gist) = cpm::with_gist_popup(|p| cpm::gist_path(&p.model)) else { return };
    let copied = match std::fs::read_to_string(&gist) {
        Ok(text) => {
            let text = text.trim();
            game.clipboard.set(text);
            !text.is_empty() && game.clipboard.get() == text
        }
        Err(e) => {
            log::warn!("cpm: cannot read {}: {e}", gist.display());
            false
        }
    };
    cpm::with_gist_popup(|p| {
        p.message = Some(if copied {
            "Copied: paste it into a new gist".to_owned()
        } else {
            cpm::copy_failed_message(p)
        });
    });
}

const WHITE: u32 = 0xFF_FF_FF;
const GREY: u32 = 0xE0_E0_E0;
const YELLOW: u32 = 0xFF_FF_80;

/// Draws the popup over the dimmed world.
pub(super) fn render(game: &Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    let Some(p) = cpm::with_gist_popup(|p| p.clone()) else { return };
    let layout = cpm::gist_layout(screen.rect);
    game.native_fill(batch, screen.rect, 0x80_00_00_00);
    let [x, y, w, h] = layout.panel;
    game.native_fill(batch, [x - 1.0, y - 1.0, w + 2.0, h + 2.0], 0xFF_80_80_80);
    game.native_fill(batch, layout.panel, 0xF0_10_10_10);
    let centred = |text: &str| x + ((w - game.native_text_width(text)) / 2.0).max(0.0);
    game.native_text(batch, cpm::GIST_TITLE, [centred(cpm::GIST_TITLE), layout.title_y], WHITE, true);
    for (i, line) in cpm::popup_lines(&p).iter().enumerate() {
        let at = [x + cpm::GIST_PAD, layout.lines_y + i as f32 * cpm::GIST_LINE_H];
        game.native_text(batch, line, at, GREY, false);
    }
    // The URL field: its tail when it is wider than the box, then the caret (blinking).
    let [fx, fy, fw, fh] = layout.field;
    game.native_fill(batch, [fx - 1.0, fy - 1.0, fw + 2.0, fh + 2.0], 0xFF_A0_A0_A0);
    game.native_fill(batch, layout.field, 0xFF_00_00_00);
    let mut shown = &p.url[..p.cursor];
    while !shown.is_empty() && game.native_text_width(shown) > fw - 8.0 {
        let mut rest = shown.chars();
        rest.next();
        shown = rest.as_str();
    }
    let text_y = fy + (fh - 8.0) / 2.0;
    game.native_text(batch, shown, [fx + 3.0, text_y], WHITE, false);
    let blink = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() / 500 % 2);
    if blink == 0 {
        let caret_x = fx + 3.0 + game.native_text_width(shown) + 1.0;
        game.native_fill(batch, [caret_x, fy + 2.0, 1.0, fh - 4.0], 0xFF_E0_E0_E0);
    }
    if let Some(message) = &p.message {
        game.native_text(batch, message, [x + cpm::GIST_PAD, layout.message_y], YELLOW, false);
    }
    for ((_, label), [bx, by, bw, bh]) in cpm::GIST_BUTTONS.iter().zip(layout.buttons) {
        let hover = cpm::inside([bx, by, bw, bh], screen.mouse);
        game.native_fill(batch, [bx, by, bw, bh], if hover { 0xFF_A0_A0_A0 } else { 0xFF_50_50_50 });
        game.native_fill(batch, [bx + 1.0, by + 1.0, bw - 2.0, bh - 2.0], if hover { 0xFF_60_60_90 } else { 0xFF_30_30_30 });
        let tx = bx + ((bw - game.native_text_width(label)) / 2.0).max(2.0);
        game.native_text(batch, label, [tx, by + (bh - 8.0) / 2.0], WHITE, true);
    }
}

/// A mouse button went down on the popup: a left click on a button runs it. Every press is kept
/// from the game.
pub(super) fn mouse_pressed(screen: &NativeScreenView<'_>, button: i32, canceled: &mut bool) {
    *canceled = true;
    if button != 0 {
        return;
    }
    match cpm::gist_button_at(&cpm::gist_layout(screen.rect), screen.mouse) {
        Some(cpm::GistButton::Copy) => cpm::request_gist(cpm::GistRequest::Copy),
        Some(cpm::GistButton::Open) => {
            if let Err(e) = open::that_detached("https://gist.github.com/") {
                log::warn!("cpm: cannot open gist.github.com: {e}");
                cpm::with_gist_popup(|p| p.message = Some(format!("Couldn't open the browser: {e}")));
            }
        }
        Some(cpm::GistButton::Save) => {
            cpm::with_gist_popup(|p| p.save_link());
        }
        Some(cpm::GistButton::Close) => cpm::request_gist(cpm::GistRequest::Close),
        None => {}
    }
}

/// A key went down on the popup: editing keys, Enter saves, Ctrl+V pastes; Escape is left to the
/// game (it closes the screen); every other key is kept from it.
pub(super) fn key_pressed(key: &str, modifiers: u32, canceled: &mut bool) {
    match cpm::gist_key(key, modifiers) {
        cpm::GistKey::Pass => return,
        cpm::GistKey::Backspace => {
            cpm::with_gist_popup(cpm::GistPopup::backspace);
        }
        cpm::GistKey::Save => {
            cpm::with_gist_popup(|p| p.save_link());
        }
        cpm::GistKey::Paste => cpm::request_gist(cpm::GistRequest::Paste),
        cpm::GistKey::Swallow => {}
    }
    *canceled = true;
}

/// A character typed on the popup goes into the URL field.
pub(super) fn char_typed(c: char, canceled: &mut bool) {
    cpm::with_gist_popup(|p| p.type_char(c));
    *canceled = true;
}
