// A click on CPM's settings page: the next model (kept as the `model` setting), the models folder,
// converting a .cpmproject (picker and conversion on their own thread), the other players'
// models' settings (on or off, how many, the safety level), or a held-back model's gist popup.
fn cpm_click(game: &mut crate::game::Game<'_>, mod_id: &str, button: usize) {
    use crate::game::cpm_mod as cpm;
    if mod_id != cpm::ID {
        return;
    }
    let data = game.native_data_dir(cpm::ID);
    // The gist cache's folder, should this click come before the frame hook's first run.
    cpm::set_data_dir(data.clone());
    let dir = cpm::models_dir(&data);
    cpm::restore_once(|| cpm::selection_from_setting(&dir, game.native_setting(cpm::ID, cpm::MODEL_SETTING)));
    // Read from disk at each click, so files added since show up; the skin's model first while
    // the skin holds one, and the cycle goes on from what draws.
    // The model loaded, as the page was built with it (its gist button).
    cpm::ensure_loaded();
    let gist = cpm::gist_button().is_some();
    match cpm::click_page(button, &cpm::model_cycle(cpm::list_models(&dir)), cpm::active_selection().as_deref(), gist) {
        cpm::Click::Select(next) => {
            game.native_set_setting(cpm::ID, cpm::MODEL_SETTING, &cpm::model_setting(next.as_deref()));
            cpm::select(next);
        }
        cpm::Click::OpenFolder => cpm::open_folder(&dir),
        // Does nothing while a conversion runs. A conversion that ended without the page showing
        // it yet (the button still said `Converting…`): this click only shows the outcome (the
        // game rebuilds the page after it); the next one starts a new picker.
        cpm::Click::Convert => {
            if !cpm::poll_convert() {
                crate::game::cpm_convert::pick_and_convert(dir, cpm::GIST_LIMIT_KIB);
            }
        }
        // The held-back model's gist popup: its screen opens once the menus are closed.
        cpm::Click::MakeGist => {
            cpm::make_gist();
        }
        // The other players' models: on or off, and how many at most (kept as settings).
        cpm::Click::OthersToggle => {
            cpm::restore_others_settings(|key| game.native_setting(cpm::ID, key));
            let on = !cpm::others_view().on;
            game.native_set_setting(cpm::ID, cpm::OTHERS_SETTING, cpm::others_setting(on));
            cpm::set_others_on(on);
        }
        cpm::Click::MaxOthers => {
            cpm::restore_others_settings(|key| game.native_setting(cpm::ID, key));
            let max = cpm::next_max_others(cpm::others_view().max);
            game.native_set_setting(cpm::ID, cpm::MAX_OTHERS_SETTING, &max.to_string());
            cpm::set_max_others(max);
        }
        // The safety level of the other players' models (kept as the `safety` setting): the
        // models it blocked load again, those past a stricter one go.
        cpm::Click::Safety => {
            cpm::restore_others_settings(|key| game.native_setting(cpm::ID, key));
            let level = cpm::others_view().safety.next();
            game.native_set_setting(cpm::ID, cpm::SAFETY_SETTING, level.setting());
            cpm::set_safety_level(level);
        }
        cpm::Click::Nothing => {}
    }
}
