// The gesture menu: CPM's own mod screen with the model's buttons, as CPM's `GestureGui` shows
// them. Added to the game as `game::cpm_menu`; its layout and state are in `cpm_mod`
// (`layout_menu`, `hit`, `Menu`), so this opens and closes the screen, draws it and turns the
// screen hooks' input (through `game::cpm_screen`, only while the menu is the CPM screen shown)
// into engine calls. The parameters a click changes are saved by the frame hook
// (`cpm_mod::take_params_to_save`). The controls take the left of the screen and a live 3D
// preview of the model the right (`cpm_mod::menu_split`, `cpm_mod::preview_quads`): a drag on it
// turns it, the wheel over it zooms. `Models` (bottom left) swaps the controls for the Models
// picker (`cpm_mod::open_picker`): the player_models files as icon tiles; a click previews that
// model on the right instead, `Use this model` selects it and `Back` returns to the gestures.
use crate::game::Game;
use crate::game::cpm_mod as cpm;
use cpm_core::anim::{Button, ButtonKind, ButtonState};
use horizon_client::mod_api::{HudBatch, NativeScreenView};

/// Once a frame: opens the screen for a menu request (the key or `Gestures…`) when the game lets
/// it (over the world with no other screen up: a request from the settings page waits until the
/// menus are closed), and drops the menu once its screen closed (Escape) or the mod was turned
/// off.
pub(super) fn frame(game: &mut Game<'_>) {
    if cpm::with_menu(|_| ()).is_some() {
        if game.native_mod_screen() != Some(cpm::ID) {
            cpm::hide_menu();
        } else if !game.native_on(cpm::ID) {
            game.native_mod_screen_close(cpm::ID);
            cpm::hide_menu();
        }
        return;
    }
    if !cpm::menu_requested() {
        return;
    }
    if !game.native_on(cpm::ID) {
        cpm::take_menu_request();
        return;
    }
    // CPM's screen (the gist popup) or another mod's is up: wait for it to close.
    if cpm::shown_screen().is_some() || game.native_mod_screen().is_some() {
        return;
    }
    if game.native_mod_screen_open(cpm::ID) {
        cpm::take_menu_request();
        cpm::show_menu();
    }
}

/// The loaded model's buttons and their states (none without a model).
fn buttons() -> (Vec<Button>, Vec<ButtonState>) {
    cpm::with_model(|m| {
        let buttons = m.engine.buttons().to_vec();
        let states = (0..buttons.len()).map(|i| m.engine.button_state(i)).collect();
        (buttons, states)
    })
    .unwrap_or_default()
}

/// What an input hook needs: the layout for this screen, the page shown (clamped to the pages
/// there are), the pointer in layout coordinates, the buttons and the menu.
struct View {
    layout: cpm::MenuLayout,
    /// The controls' width and the preview's rect (layout coordinates).
    controls_w: f32,
    preview: [f32; 4],
    page: usize,
    mouse: [f32; 2],
    buttons: Vec<Button>,
    menu: cpm::Menu,
    gui_h: f32,
}

fn view(screen: &NativeScreenView<'_>) -> Option<View> {
    let menu = cpm::with_menu(|m| m.clone())?;
    let [ox, oy, w, h] = screen.rect;
    let (buttons, _) = buttons();
    let (controls_w, preview) = cpm::menu_split(w, h);
    let layout = cpm::layout_menu(&buttons, controls_w, h);
    let page = menu.page.min(layout.pages.len().saturating_sub(1));
    let mouse = [screen.mouse[0] - ox, screen.mouse[1] - oy];
    Some(View { layout, controls_w, preview, page, mouse, buttons, menu, gui_h: h })
}

impl View {
    /// Button `i`'s cell on the page shown.
    fn widget(&self, i: usize) -> Option<&cpm::Widget> {
        self.layout.pages.get(self.page)?.iter().find(|w| w.button == i)
    }

    /// The open dropdown, its cell and its list's rows.
    fn open_list(&self) -> Option<(usize, &[String], Vec<[f32; 4]>)> {
        let i = self.menu.open_dropdown?;
        let ButtonKind::Dropdown { options } = &self.buttons.get(i)?.kind else { return None };
        let cell = self.widget(i)?.rect;
        Some((i, options.as_slice(), cpm::dropdown_list(cell, options.len(), self.gui_h)))
    }

    /// Slider `i` set from the pointer.
    fn drag_slider(&self, i: usize) {
        let Some(ButtonKind::Slider { max }) = self.buttons.get(i).map(|b| &b.kind) else { return };
        let Some(w) = self.widget(i) else { return };
        let value = cpm::slider_value(w, self.mouse[0], *max);
        cpm::with_model(|m| m.engine.set_value(i, value));
    }
}

const WHITE: u32 = 0xFF_FF_FF;
const GREY: u32 = 0xE0_E0_E0;
const YELLOW: u32 = 0xFF_FF_80;
const RED: u32 = 0xFF_60_60;

/// `text` cut (ending in "...") to fit `width`.
fn fit(game: &Game<'_>, text: &str, width: f32) -> String {
    if game.native_text_width(text) <= width {
        return text.to_owned();
    }
    let mut cut = text.to_owned();
    while !cut.is_empty() && game.native_text_width(&format!("{cut}...")) > width {
        cut.pop();
    }
    format!("{cut}...")
}

/// `text` centred in `rect` (vertically on its middle, or at `y` when given).
fn centred(game: &Game<'_>, batch: &mut HudBatch, text: &str, rect: [f32; 4], y: Option<f32>, rgb: u32) {
    let [x, ry, w, h] = rect;
    let text = fit(game, text, w - 4.0);
    let tx = x + ((w - game.native_text_width(&text)) / 2.0).max(2.0);
    game.native_text(batch, &text, [tx, y.unwrap_or(ry + (h - 8.0) / 2.0)], rgb, true);
}

/// A button's frame: a border and its fill (lighter when hovered, green when active).
fn frame_box(game: &Game<'_>, batch: &mut HudBatch, rect: [f32; 4], hover: bool, active: bool) {
    let [x, y, w, h] = rect;
    let (border, fill) = match (active, hover) {
        (true, true) => (0xFF_A0_E0_A0, 0xFF_40_88_40),
        (true, false) => (0xFF_60_C0_60, 0xFF_2E_6E_2E),
        (false, true) => (0xFF_A0_A0_A0, 0xFF_60_60_90),
        (false, false) => (0xFF_50_50_50, 0xFF_30_30_30),
    };
    game.native_fill(batch, rect, border);
    game.native_fill(batch, [x + 1.0, y + 1.0, w - 2.0, h - 2.0], fill);
}

fn button(game: &Game<'_>, batch: &mut HudBatch, rect: [f32; 4], label: &str, hover: bool, active: bool) {
    frame_box(game, batch, rect, hover, active);
    centred(game, batch, label, rect, None, WHITE);
}

/// Draws the menu over the dimmed world.
pub(super) fn render(game: &Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    let Some(v) = view(screen) else { return };
    if cpm::picker_shown() {
        return render_picker(game, batch, screen, &v);
    }
    let (_, states) = buttons();
    let [ox, oy, _, _] = screen.rect;
    let w = v.controls_w;
    let at = |r: [f32; 4]| [r[0] + ox, r[1] + oy, r[2], r[3]];
    let list = v.open_list();
    // With a list open only the list hovers.
    let hover = |r: [f32; 4]| list.is_none() && cpm::inside(r, v.mouse);
    let l = &v.layout;
    game.native_fill(batch, screen.rect, 0x80_00_00_00);
    preview(game, batch, at(v.preview), &v.menu.preview);
    centred(game, batch, cpm::MENU_TITLE, at([0.0, 0.0, w, 0.0]), Some(oy + l.title_y), WHITE);
    button(game, batch, at(l.reset_gesture), cpm::RESET_GESTURE, hover(l.reset_gesture), false);
    button(game, batch, at(l.reset_pose), cpm::RESET_POSE, hover(l.reset_pose), false);
    if l.pages.is_empty() {
        centred(game, batch, cpm::NO_GESTURES, at([0.0, l.grid_y, w, 20.0]), None, GREY);
    }
    for widget in l.pages.get(v.page).into_iter().flatten() {
        let (Some(b), Some(state)) = (v.buttons.get(widget.button), states.get(widget.button)) else { continue };
        let rect = at(widget.rect);
        let [x, y, cw, ch] = rect;
        let hovered = hover(widget.rect) || v.menu.dragging == Some(widget.button);
        match (&b.kind, *state) {
            (ButtonKind::Toggle, ButtonState::Active(on)) => {
                // CPM's rocker (GestureToggleButton): the left half green when on, the right half
                // red when off, the other half the button's own fill.
                frame_box(game, batch, rect, hovered, false);
                let fill = if hovered { 0xFF_60_60_90 } else { 0xFF_30_30_30 };
                let half = ((cw - 2.0) / 2.0).floor();
                game.native_fill(batch, [x + 1.0, y + 1.0, half, ch - 2.0], if on { 0xFF_00_FF_00 } else { fill });
                game.native_fill(batch, [x + 1.0 + half, y + 1.0, cw - 2.0 - half, ch - 2.0], if on { fill } else { 0xFF_FF_00_00 });
                centred(game, batch, &b.name, rect, None, WHITE);
            }
            (ButtonKind::Slider { max }, ButtonState::Value(value)) => {
                game.native_fill(batch, rect, if hovered { 0xFF_FF_FF_FF } else { 0xFF_A0_A0_A0 });
                game.native_fill(batch, [x + 1.0, y + 1.0, cw - 2.0, ch - 2.0], 0xFF_00_00_00);
                let mx = cpm::slider_marker_x(widget, value, *max) + ox;
                frame_box(game, batch, [mx, y, cpm::SLIDER_MARKER_W, ch], hovered, false);
                centred(game, batch, &format!("{}: {value}", b.name), rect, None, WHITE);
            }
            (ButtonKind::Dropdown { options }, ButtonState::Selected(selected)) => {
                game.native_text(batch, &fit(game, &b.name, cw), [x, y + 1.0], GREY, false);
                let field = [x, y + cpm::DROPDOWN_LABEL_H, cw, ch - cpm::DROPDOWN_LABEL_H];
                let open = v.menu.open_dropdown == Some(widget.button);
                game.native_fill(batch, field, if hovered || open { 0xFF_FF_FF_FF } else { 0xFF_A0_A0_A0 });
                game.native_fill(batch, [x + 1.0, field[1] + 1.0, cw - 2.0, field[3] - 2.0], 0xFF_00_00_00);
                let current = options.get(selected).map_or("", |o| cpm::option_label(o));
                let text_y = field[1] + (field[3] - 8.0) / 2.0;
                game.native_text(batch, &fit(game, current, cw - 16.0), [x + 4.0, text_y], WHITE, false);
                game.native_text(batch, "v", [x + cw - 10.0, text_y], WHITE, false);
            }
            // Gestures and custom poses: green while playing / selected.
            (_, state) => {
                button(game, batch, rect, &b.name, hovered, state == ButtonState::Active(true));
            }
        }
    }
    if let (Some(prev), Some(next)) = (l.prev, l.next) {
        button(game, batch, at(prev), "<", hover(prev), false);
        button(game, batch, at(next), ">", hover(next), false);
        let label = format!("{} / {}", v.page + 1, l.pages.len());
        let between = [prev[0] + prev[2], prev[1], next[0] - prev[0] - prev[2], prev[3]];
        centred(game, batch, &label, at(between), None, GREY);
    }
    let models = cpm::models_button(v.gui_h);
    button(game, batch, at(models), cpm::MODELS, hover(models), false);
    // The open dropdown's list, over everything else.
    if let Some((i, options, rows)) = &list {
        let selected = match states.get(*i) {
            Some(ButtonState::Selected(s)) => *s,
            _ => 0,
        };
        if let (Some(first), Some(last)) = (rows.first(), rows.last()) {
            let [x, y, cw, _] = at(*first);
            let bottom = last[1] + last[3] + oy;
            game.native_fill(batch, [x - 1.0, y - 1.0, cw + 2.0, bottom - y + 2.0], 0xFF_FF_FF_FF);
        }
        for (k, (row, option)) in rows.iter().zip(options.iter()).enumerate() {
            let rect = at(*row);
            let hovered = cpm::inside(*row, v.mouse);
            game.native_fill(batch, rect, if hovered { 0xFF_40_40_60 } else { 0xFF_00_00_00 });
            let rgb = if k == selected { YELLOW } else { WHITE };
            let text = fit(game, cpm::option_label(option), rect[2] - 8.0);
            game.native_text(batch, &text, [rect[0] + 4.0, rect[1] + (rect[3] - 8.0) / 2.0], rgb, false);
        }
    }
}

/// The model posed live (the world draw's input at this time, wearing what the player wears) in
/// `rect` (GUI pixels), as Horizon draws its inventory player: depth-tested quads clipped to the
/// rect. Nothing without a model, before its texture reached the game, or when the atlas lacks the
/// model's sprite; a face of a worn texture the atlas lacks is left out.
fn preview(game: &Game<'_>, batch: &mut HudBatch, rect: [f32; 4], view: &cpm::PreviewView) {
    game.native_fill(batch, rect, 0x40_00_00_00);
    // The clock first: it and the model share the runtime's state.
    let input = cpm::preview_input(cpm::clock_ms(std::time::Instant::now()));
    let worn = crate::game::cpm_equipment_probe::worn(game);
    let Some(quads) = cpm::preview_quads(&input, &worn, view, rect) else { return };
    draw_preview(game, batch, rect, &quads);
}

/// `quads` drawn depth-tested, clipped to `rect`; nothing when the atlas lacks the model's sprite,
/// and a face of a sprite it lacks is left out.
fn draw_preview(game: &Game<'_>, batch: &mut HudBatch, rect: [f32; 4], quads: &cpm::PreviewQuads) {
    let [x, y, w, h] = rect;
    let atlas = &game.gui.atlas.sprites;
    let sprites: Vec<_> = quads.sprites.iter().map(|s| atlas.get(s).copied()).collect();
    if sprites.first().copied().flatten().is_none() {
        return;
    }
    let start = batch.vertices.len();
    for q in &quads.quads {
        let Some(r) = sprites.get(q.sprite).copied().flatten() else { continue };
        let uv = q.uv.map(|[u, v]| [r.u0 + (r.u1 - r.u0) * u / 16.0, r.v0 + (r.v1 - r.v0) * v / 16.0]);
        batch.quad_3d(q.corners, uv, q.color);
    }
    batch.depth_since(start, [x, y, x + w, y + h]);
}

/// The picker page: the title, the tiles of the page shown (an icon, or the name for a file
/// without one; the name under it, red when the file does not read), the hovered tile's full name
/// (and why it does not read), `Back`, `Use this model`, the page buttons, and the clicked model's
/// preview on the right.
fn render_picker(game: &Game<'_>, batch: &mut HudBatch, screen: &NativeScreenView<'_>, v: &View) {
    let [ox, oy, _, _] = screen.rect;
    let at = |r: [f32; 4]| [r[0] + ox, r[1] + oy, r[2], r[3]];
    let hover = |r: [f32; 4]| cpm::inside(r, v.mouse);
    let l = cpm::picker_layout(v.controls_w, v.gui_h);
    let tiles = cpm::picker_tiles();
    let grid = cpm::picker_grid(tiles.len(), l.grid);
    let (page, chosen) = cpm::picker_page();
    let page = page.min(grid.pages.len().saturating_sub(1));
    game.native_fill(batch, screen.rect, 0x80_00_00_00);
    picker_preview(game, batch, at(v.preview), &v.menu.preview);
    centred(game, batch, cpm::MODELS, at([0.0, 0.0, v.controls_w, 0.0]), Some(oy + l.title_y), WHITE);
    let mut hovered = None;
    for (k, rect) in grid.pages.get(page).into_iter().flatten().enumerate() {
        let i = page * grid.per_page + k;
        let Some(tile) = tiles.get(i) else { continue };
        let [x, y, w, h] = at(*rect);
        if hover(*rect) {
            hovered = Some(tile);
        }
        frame_box(game, batch, [x, y, w, h], hover(*rect), chosen == Some(i));
        let px = cpm::ICON_PX as f32;
        let icon = [x + (w - px) / 2.0, y + 2.0, px, px];
        let drawn = tile
            .icon_at
            .is_some_and(|[sx, sy]| game.native_sprite(batch, cpm::SPRITE, [sx as f32, sy as f32, px, px], icon));
        if !drawn {
            // A name tile.
            centred(game, batch, &tile.name, icon, None, GREY);
        }
        let rgb = if tile.error.is_some() { RED } else { WHITE };
        centred(game, batch, &tile.name, [x, y + h - 10.0, w, 8.0], None, rgb);
    }
    if tiles.is_empty() {
        centred(game, batch, cpm::NO_MODELS, at([0.0, l.grid[1], v.controls_w, 20.0]), None, GREY);
    }
    // The hovered tile in full, over the bottom row.
    if let Some(tile) = hovered {
        let text = match &tile.error {
            Some(e) => format!("{}: {e}", tile.name),
            None => tile.name.clone(),
        };
        let rgb = if tile.error.is_some() { RED } else { GREY };
        let y = l.back[1] - 10.0;
        game.native_text(batch, &fit(game, &text, l.grid[2]), [ox + l.grid[0], oy + y], rgb, true);
    }
    button(game, batch, at(l.back), cpm::BACK, hover(l.back), false);
    button(game, batch, at(l.use_model), cpm::USE_MODEL, chosen.is_some() && hover(l.use_model), false);
    if grid.pages.len() > 1 {
        button(game, batch, at(l.prev), "<", hover(l.prev), false);
        button(game, batch, at(l.next), ">", hover(l.next), false);
        let label = format!("{} / {}", page + 1, grid.pages.len());
        let left = l.use_model[0] + l.use_model[2];
        let between = [left, l.prev[1], l.prev[0] - left, l.prev[3]];
        centred(game, batch, &label, at(between), None, GREY);
    }
}

/// The clicked model standing in `rect`, at its default parameters and wearing what the player
/// wears; why not when it cannot be shown; the old-conversion note under it.
fn picker_preview(game: &Game<'_>, batch: &mut HudBatch, rect: [f32; 4], view: &cpm::PreviewView) {
    game.native_fill(batch, rect, 0x40_00_00_00);
    match cpm::picker_preview_status() {
        cpm::PreviewStatus::Ready => {
            let now = cpm::clock_ms(std::time::Instant::now());
            let worn = crate::game::cpm_equipment_probe::worn(game);
            if let Some(quads) = cpm::picker_preview_quads(&worn, view, rect, now) {
                draw_preview(game, batch, rect, &quads);
            }
        }
        cpm::PreviewStatus::Message(m) => centred(game, batch, &m, rect, None, GREY),
        cpm::PreviewStatus::Nothing | cpm::PreviewStatus::Loading => {}
    }
    if let Some(note) = cpm::picker_note() {
        let [x, y, w, h] = rect;
        centred(game, batch, &note, [x, y + h - 14.0, w, 12.0], None, YELLOW);
    }
}

/// A left click on the picker page: `Back`, `Use this model` (keeps the `model` setting as the
/// settings page's Model button does, then the gestures show), a page button, a tile (previews
/// it) or the preview (a drag turns it).
fn picker_pressed(game: &Game<'_>, v: &View, button: i32) {
    if button != 0 {
        return;
    }
    let l = cpm::picker_layout(v.controls_w, v.gui_h);
    let grid = cpm::picker_grid(cpm::picker_tiles().len(), l.grid);
    let pages = grid.pages.len();
    let page = cpm::picker_page().0.min(pages.saturating_sub(1));
    let on = |r: [f32; 4]| cpm::inside(r, v.mouse);
    if on(l.back) {
        cpm::close_picker();
    } else if on(l.use_model) {
        if let Some(name) = cpm::use_previewed() {
            game.native_set_setting(cpm::ID, cpm::MODEL_SETTING, &name);
            cpm::with_menu(|m| m.page = 0);
        }
    } else if pages > 1 && on(l.prev) {
        cpm::set_picker_page(page.saturating_sub(1));
    } else if pages > 1 && on(l.next) {
        cpm::set_picker_page((page + 1).min(pages - 1));
    } else if let Some(i) = grid.hit(page, v.mouse) {
        cpm::preview_select(i);
    } else if on(v.preview) {
        cpm::with_menu(|m| m.rotating = true);
    }
}

/// A mouse button went down on the menu: a left click runs what it is on (an open dropdown list
/// takes the click first: an option is chosen, anywhere else closes it). Every press is kept
/// from the game.
pub(super) fn mouse_pressed(game: &Game<'_>, screen: &NativeScreenView<'_>, button: i32, canceled: &mut bool) {
    *canceled = true;
    // A release that never came must not hold the saves back until the menu closes.
    cpm::with_menu(|m| {
        m.dragging = None;
        m.rotating = false;
    });
    let Some(v) = view(screen) else { return };
    if cpm::picker_shown() {
        return picker_pressed(game, &v, button);
    }
    if let Some((i, _, rows)) = v.open_list() {
        if button == 0
            && let Some(option) = cpm::option_at(&rows, v.mouse)
        {
            cpm::with_model(|m| m.engine.select(i, option));
        }
        cpm::with_menu(|m| m.open_dropdown = None);
        return;
    }
    if button != 0 {
        return;
    }
    let pages = v.layout.pages.len();
    match cpm::hit(&v.layout, v.page, v.mouse[0], v.mouse[1]) {
        Some(cpm::Hit::ResetGesture) => {
            cpm::with_model(|m| m.engine.reset_gesture());
        }
        Some(cpm::Hit::ResetPose) => {
            cpm::with_model(|m| m.engine.reset_pose());
        }
        Some(cpm::Hit::Prev) => {
            cpm::with_menu(|m| m.page = v.page.saturating_sub(1));
        }
        Some(cpm::Hit::Next) => {
            cpm::with_menu(|m| m.page = (v.page + 1).min(pages.saturating_sub(1)));
        }
        Some(cpm::Hit::Button(i)) => match v.buttons.get(i).map(|b| &b.kind) {
            Some(ButtonKind::Slider { .. }) => {
                v.drag_slider(i);
                cpm::with_menu(|m| m.dragging = Some(i));
            }
            Some(ButtonKind::Dropdown { .. }) => {
                cpm::with_menu(|m| m.open_dropdown = Some(i));
            }
            Some(_) => {
                // The clock first: it and the model share the runtime's state.
                let now = cpm::clock_ms(std::time::Instant::now());
                cpm::with_model(|m| m.engine.press(i, now));
            }
            None => {}
        },
        None if cpm::inside(cpm::models_button(v.gui_h), v.mouse) => {
            cpm::open_picker(&cpm::models_dir(&game.native_data_dir(cpm::ID)));
        }
        // A drag that begins on the preview turns it.
        None if cpm::inside(v.preview, v.mouse) => {
            cpm::with_menu(|m| m.rotating = true);
        }
        None => {}
    }
}

/// The left button dragged: the slider it went down on follows the pointer, or the preview it
/// went down on turns by `delta` (GUI pixels).
pub(super) fn mouse_dragged(screen: &NativeScreenView<'_>, button: i32, delta: [f32; 2], canceled: &mut bool) {
    *canceled = true;
    let Some(v) = view(screen) else { return };
    if button != 0 {
        return;
    }
    if let Some(i) = v.menu.dragging {
        v.drag_slider(i);
    } else if v.menu.rotating {
        cpm::with_menu(|m| m.preview.drag(delta[0], delta[1]));
    }
}

/// The wheel over the preview zooms it; over the picker's tiles it turns their pages. The menu
/// keeps every turn of the wheel from the game.
pub(super) fn mouse_scrolled(screen: &NativeScreenView<'_>, scroll: [f64; 2], canceled: &mut bool) {
    *canceled = true;
    let Some(v) = view(screen) else { return };
    if cpm::inside(v.preview, v.mouse) {
        cpm::with_menu(|m| m.preview.scroll(scroll[1] as f32));
    } else if cpm::picker_shown() && scroll[1] != 0.0 {
        let l = cpm::picker_layout(v.controls_w, v.gui_h);
        if cpm::inside(l.grid, v.mouse) {
            let pages = cpm::picker_grid(cpm::picker_tiles().len(), l.grid).pages.len();
            let page = cpm::picker_page().0.min(pages.saturating_sub(1));
            let next = if scroll[1] < 0.0 { (page + 1).min(pages.saturating_sub(1)) } else { page.saturating_sub(1) };
            cpm::set_picker_page(next);
        }
    }
}

/// A button went up: the drag (a slider's or the preview's) ends. Kept from the game, as its
/// press was.
pub(super) fn mouse_released(button: i32, canceled: &mut bool) {
    *canceled = true;
    if button == 0 {
        cpm::with_menu(|m| {
            m.dragging = None;
            m.rotating = false;
        });
    }
}

/// Escape is left to the game (it closes the screen); every other key is kept from it.
pub(super) fn key_pressed(key: &str, canceled: &mut bool) {
    if key != "key.keyboard.escape" {
        *canceled = true;
    }
}
