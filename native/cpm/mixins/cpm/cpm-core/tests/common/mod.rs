//! Shared by the tests that read the sample models in `tests/samples/` (not distributed; see the README there).
#![allow(dead_code)]

/// True (after saying so on stderr) when any of the named sample files is absent, so the test returns early.
pub fn missing(names: &[&str]) -> bool {
    for name in names {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/samples").join(name);
        if !path.is_file() {
            eprintln!("skipping: sample {name} not present");
            return true;
        }
    }
    false
}
