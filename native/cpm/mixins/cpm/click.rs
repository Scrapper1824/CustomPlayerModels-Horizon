// A click on CPM's settings page: the next model (kept as the `model` setting), the models folder,
// converting a .cpmproject (picker and conversion on their own thread), the next gist limit, or
// the other players' models' settings.
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
    let limit = cpm::gist_limit(game.native_setting(cpm::ID, cpm::GIST_LIMIT_SETTING));
    // Read from disk at each click, so files added since show up; the skin's model first while
    // the skin holds one, and the cycle goes on from what draws.
    match cpm::click(button, &cpm::model_cycle(cpm::list_models(&dir)), cpm::active_selection().as_deref()) {
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
                crate::game::cpm_convert::pick_and_convert(dir, limit);
            }
        }
        cpm::Click::GistLimit => {
            game.native_set_setting(cpm::ID, cpm::GIST_LIMIT_SETTING, &cpm::next_gist_limit(limit).to_string());
        }
        cpm::Click::Gestures => cpm::request_menu(),
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
        cpm::Click::Nothing => {}
    }
}
