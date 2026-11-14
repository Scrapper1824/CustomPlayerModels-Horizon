// CPM's settings page: the model, the models folder, the converter (its button, the gist limit and
// the last result), the other players' models (on or off, how many, the safety level, how many
// draw and how many it blocks), a held-back model's `Make a gist link…`, the last error and what
// the others see of the model.
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
        old_conversion: cpm::old_conversion(),
    };
    // The other players' models: their three settings, how many draw and how many are blocked.
    cpm::restore_others_settings(|key| game.native_setting(cpm::ID, key));
    let gist = cpm::gist_button();
    buttons.extend(cpm::settings_page(selected.as_deref(), error.as_deref(), &convert, &cpm::others_view(), gist.as_deref()));
    // In a Horizon-hosted world: what the other players see of the model, and why not, when not.
    buttons.extend(cpm::net_status_line());
}
