// A link's download (a gist, or a link of CPM's other loaders), on a thread of its own so a slow
// network never holds up the game. Added to the game as `game::cpm_gist_fetch`; the frame hook
// starts the downloads `cpm_mod::take_fetch_requests` hands it, and the outcome goes back through
// `cpm_mod::finish_fetch` (the frame hook then caches and loads it).
use std::io::Read;
use std::time::Duration;

/// How long the download may take.
const TIMEOUT: Duration = Duration::from_secs(15);
/// The most text taken, in bytes.
const MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Downloads `link`'s text on a new thread from `urls` (`cpm_core::link::link_urls`): the next URL
/// is tried only when one does not connect or times out (`PasteResourceLoader`'s fallback), then
/// hands the text, or why there is none, to `finish_fetch`.
pub fn fetch(link: String, urls: Vec<String>) {
    use crate::game::cpm_mod as cpm;
    let thread_link = link.clone();
    let spawned = std::thread::Builder::new().name("cpm-gist".into()).spawn(move || {
        // A panic must still end the download, or the link would stay in flight for good.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| download_any(&urls)))
            .unwrap_or_else(|_| Err("download crashed".to_owned()));
        match &result {
            Ok(text) => log::info!("cpm: downloaded {thread_link} ({} bytes)", text.len()),
            Err(e) => log::debug!("cpm: link {thread_link}: {e}"),
        }
        cpm::finish_fetch(&thread_link, result);
    });
    if let Err(e) = spawned {
        cpm::finish_fetch(&link, Err(format!("cannot start the download: {e}")));
    }
}

/// The first of `urls` that connects: an HTTP error answer is final, a transport error (no
/// connection, timeout) moves on to the next URL.
fn download_any(urls: &[String]) -> Result<Vec<u8>, String> {
    let mut last = "no URL".to_owned();
    for url in urls {
        match download(url) {
            Ok(body) => return Ok(body),
            Err((e, transport)) => {
                last = e;
                if !transport {
                    break;
                }
            }
        }
    }
    Err(last)
}

/// The body at `url`, at most [`MAX_BYTES`], gzip accepted (`HttpResourceLoader`). The error says
/// whether it was a transport one.
fn download(url: &str) -> Result<Vec<u8>, (String, bool)> {
    let agent = ureq::AgentBuilder::new().timeout(TIMEOUT).build();
    let response = agent.get(url).set("Accept-Encoding", "gzip").call().map_err(|e| {
        let transport = matches!(e, ureq::Error::Transport(_));
        (e.to_string(), transport)
    })?;
    let gzip = response.header("Content-Encoding").is_some_and(|v| v.eq_ignore_ascii_case("gzip"));
    let reader: Box<dyn Read> = if gzip {
        Box::new(flate2::read::GzDecoder::new(response.into_reader()))
    } else {
        Box::new(response.into_reader())
    };
    let mut body = Vec::new();
    reader.take(MAX_BYTES + 1).read_to_end(&mut body).map_err(|e| (e.to_string(), false))?;
    if body.len() as u64 > MAX_BYTES {
        return Err((format!("larger than {} MiB", MAX_BYTES / (1024 * 1024)), false));
    }
    Ok(body)
}
