// A link model's gist download, on a thread of its own so a slow network never holds up the
// game. Added to the game as `game::cpm_gist_fetch`; the frame hook starts the downloads
// `cpm_mod::take_fetch_requests` hands it, and the outcome goes back through
// `cpm_mod::finish_fetch` (the frame hook then caches and loads it).
use std::io::Read;
use std::time::Duration;

/// How long the download may take.
const TIMEOUT: Duration = Duration::from_secs(15);
/// The most gist text taken, in bytes.
const MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Downloads `raw_url` (the gist text of `link`) on a new thread, then hands the text, or why
/// there is none, to `finish_fetch`.
pub fn fetch(link: String, raw_url: String) {
    use crate::game::cpm_mod as cpm;
    let thread_link = link.clone();
    let spawned = std::thread::Builder::new().name("cpm-gist".into()).spawn(move || {
        // A panic must still end the download, or the link would stay in flight for good.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| download(&raw_url)))
            .unwrap_or_else(|_| Err("download crashed".to_owned()));
        match &result {
            Ok(text) => log::info!("cpm: downloaded gist {thread_link} ({} bytes)", text.len()),
            Err(e) => log::debug!("cpm: gist {thread_link}: {e}"),
        }
        cpm::finish_fetch(&thread_link, result);
    });
    if let Err(e) = spawned {
        cpm::finish_fetch(&link, Err(format!("cannot start the download: {e}")));
    }
}

/// The body at `url`, at most [`MAX_BYTES`].
fn download(url: &str) -> Result<Vec<u8>, String> {
    let response = ureq::AgentBuilder::new().timeout(TIMEOUT).build().get(url).call().map_err(|e| e.to_string())?;
    let mut body = Vec::new();
    response.into_reader().take(MAX_BYTES + 1).read_to_end(&mut body).map_err(|e| e.to_string())?;
    if body.len() as u64 > MAX_BYTES {
        return Err(format!("larger than {} MiB", MAX_BYTES / (1024 * 1024)));
    }
    Ok(body)
}
