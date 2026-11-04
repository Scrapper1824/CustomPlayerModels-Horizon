// CPM's settings page: the model, the models folder, the converter (its button, the gist limit and
// the last result), `Gestures…` and the last error.
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
    let (selected, error, last) = (cpm::selected(), cpm::last_error(), cpm::last_convert());
    let convert = cpm::ConvertView {
        busy: cpm::convert_busy(),
        last: last.as_ref(),
        limit_kib: cpm::gist_limit(game.native_setting(cpm::ID, cpm::GIST_LIMIT_SETTING)),
        old_conversion: cpm::old_conversion(),
    };
    buttons.extend(cpm::settings_labels(selected.as_deref(), error.as_deref(), &convert));
}
