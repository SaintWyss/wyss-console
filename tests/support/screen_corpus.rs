use std::path::PathBuf;

pub fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/screen")
}

pub fn names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir())
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            let n = p.file_name()?.to_str()?;
            n.strip_suffix(".bin").map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}
