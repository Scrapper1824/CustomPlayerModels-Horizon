// The settings page's `Convert project…`: the system's file picker, then the conversion, on a
// thread of their own so neither the dialog nor a big file holds up the game. Added to the game as
// `game::cpm_convert`; the outcome goes back through `cpm_mod::finish_convert_job`.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Shown when no picker program could be started.
const NO_PICKER: &str = "No file picker available: put the .cpmmodel in the player_models folder yourself";

/// Asks for a `.cpmproject` (starting in `models_dir`) and converts it into `models_dir` with a
/// gist limit of `limit` KiB, on a new thread. Nothing when a conversion already runs.
pub fn pick_and_convert(models_dir: PathBuf, limit: u32) {
    use crate::game::cpm_mod as cpm;
    if !cpm::start_convert_job() {
        return;
    }
    let spawned = std::thread::Builder::new().name("cpm-convert".into()).spawn(move || {
        // A panic must still end the job, or the button would say `Converting…` for good.
        let run = std::panic::AssertUnwindSafe(|| match pick(&models_dir) {
            Ok(None) => None,
            Ok(Some(project)) => Some(cpm::convert_project(&project, &models_dir, limit)),
            Err(e) => Some(cpm::ConvertResult::Failed(e)),
        });
        let result = std::panic::catch_unwind(run).unwrap_or_else(|_| {
            log::warn!("cpm: the converter crashed");
            Some(cpm::ConvertResult::Failed("converter crashed".into()))
        });
        if let Some(cpm::ConvertResult::Saved { model, size, .. }) = &result {
            log::info!("cpm: converted {} ({size} bytes)", model.display());
        }
        cpm::finish_convert_job(result);
    });
    if let Err(e) = spawned {
        cpm::finish_convert_job(Some(cpm::ConvertResult::Failed(format!("cannot start the converter: {e}"))));
    }
}

/// Rebuilds CPM's settings page (its row in Options > Mods), which the game otherwise rebuilds
/// only when it opens and after a click: the frame hook calls it when a conversion ended, so the
/// page drops `Converting…` and shows the outcome by itself.
pub(super) fn refresh_page(game: &mut crate::game::Game<'_>) {
    use crate::game::cpm_mod as cpm;
    let row = game.settings.mods.installed.iter().position(|m| m.id == cpm::ID && m.native.is_some());
    if let Some(i) = row {
        game.native_settings_refresh(i);
    }
}

/// The picked file; `None` when the dialog was cancelled (empty output).
fn pick(dir: &Path) -> Result<Option<PathBuf>, String> {
    let output = run_picker(dir).ok_or_else(|| NO_PICKER.to_owned())?;
    let success = output.status.success();
    if !success {
        // Cancelling exits non-zero for zenity, kdialog and osascript: only worth a debug line.
        log::debug!("cpm: file picker: {} {}", output.status, String::from_utf8_lossy(&output.stderr).trim());
    }
    crate::game::cpm_mod::picker_outcome(&output.stdout, success, &output.stderr, cfg!(windows))
        .map_err(|e| format!("file picker: {e}"))
}

/// Runs the platform's picker and waits for it; `None` when no picker program could be started.
#[cfg(windows)]
fn run_picker(dir: &Path) -> Option<Output> {
    use std::os::windows::process::CommandExt;
    /// `CREATE_NO_WINDOW`: no console window flashes up behind the dialog.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    use crate::game::cpm_mod as cpm;
    let mut command = Command::new("powershell");
    command
        .args(["-NoProfile", "-STA", "-Command", &cpm::powershell_picker_script()])
        .env(cpm::PICKER_DIR_ENV, dir)
        .creation_flags(CREATE_NO_WINDOW);
    output(command)
}

#[cfg(target_os = "macos")]
fn run_picker(dir: &Path) -> Option<Output> {
    let dir = dir.display().to_string().replace('\\', "\\\\").replace('"', "\\\"");
    let script = format!("POSIX path of (choose file of type {{\"cpmproject\"}} default location \"{dir}\")");
    let mut command = Command::new("osascript");
    command.args(["-e", &script]);
    output(command)
}

#[cfg(not(any(windows, target_os = "macos")))]
fn run_picker(dir: &Path) -> Option<Output> {
    let mut zenity = Command::new("zenity");
    zenity.args(["--file-selection", "--file-filter=*.cpmproject"]).arg(format!("--filename={}/", dir.display()));
    output(zenity).or_else(|| {
        let mut kdialog = Command::new("kdialog");
        kdialog.arg("--getopenfilename").arg(dir).arg("*.cpmproject");
        output(kdialog)
    })
}

/// Runs `command` to the end; `None` (logged) when it cannot be started.
fn output(mut command: Command) -> Option<Output> {
    match command.output() {
        Ok(output) => Some(output),
        Err(e) => {
            log::warn!("cpm: cannot start {:?}: {e}", command.get_program());
            None
        }
    }
}
