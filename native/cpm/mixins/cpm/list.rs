// CPM's row in Options > Mods, with its gesture menu key.
fn cpm_listed(mods: &mut Vec<crate::game::native_hooks::NativeMod>) {
    use crate::game::cpm_mod as cpm;
    mods.push(crate::game::native_hooks::NativeMod {
        id: cpm::ID.into(),
        title: format!("CPM {}", cpm::VERSION),
        description: cpm::DESCRIPTION.into(),
        keys: cpm::keys(),
    });
}
