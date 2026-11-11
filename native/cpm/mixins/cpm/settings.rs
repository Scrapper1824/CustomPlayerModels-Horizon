// CPM's settings page: the model, the models folder, the converter (its button, the gist limit and
// the last result), `Gestures…`, the other players' models (on or off, how many, how many draw)
// and the last error.
fn cpm_page(game: &mut crate::game::Game<'_>, mod_id: &str, buttons: &mut Vec<String>) {
    use crate::game::cpm_mod as cpm;
    if mod_id != cpm::ID {
        return;
    }
    cpm::restore_once(|| {
        let data = game.native_data_dir(cpm::ID);
        cpm::set_data_dir(data.clone());
        let dir = cpm::models_dir(&data);
        cpm::selection_from_setting(&dir, game.native_setting(cpm::ID, cpm::MODEL_SETTING))
    });
    // A click may just have changed the model: load it so its errors show.
    cpm::ensure_loaded();
    // A conversion that just ended shows here even when the frame hook is not running.
    let _ = cpm::poll_convert();
    // What draws: the skin's model too when nothing is chosen (its own line says so).
    let (selected, error, last) = (cpm::active_selection(), cpm::last_error(), cpm::last_convert());
    let convert = cpm::ConvertView {
        busy: cpm::convert_busy(),
        last: last.as_ref(),
        limit_kib: cpm::gist_limit(game.native_setting(cpm::ID, cpm::GIST_LIMIT_SETTING)),
        old_conversion: cpm::old_conversion(),
    };
    // The other players' models: their two settings and how many draw.
    cpm::restore_others_settings(|key| game.native_setting(cpm::ID, key));
    buttons.extend(cpm::settings_page(selected.as_deref(), error.as_deref(), &convert, &cpm::others_view()));
}
