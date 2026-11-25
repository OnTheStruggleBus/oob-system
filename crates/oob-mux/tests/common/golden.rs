//! Golden-vector loader shared by the box-free tests (#17 PSI vectors,
//! #18 stream vectors).
//!
//! A vector is a hex text file under `tests/golden/`: `#` starts a comment
//! line (the header), everything else is whitespace-separated hex bytes.
//! Every file's header must open with the status line
//! `status: hypothesized — regression lock, not box-verified` (AGENTS.md §2):
//! a vector made by this crate proves only that the bytes did not change.
//! Only a capture in `lab/captures/` promotes status.
//!
//! `OOB_BLESS=1` rewrites a vector after a deliberate change, keeping its
//! header (a new file gets a stub header that the author must complete).
#![allow(dead_code)]

use std::path::PathBuf;

/// The status line every golden header carries.
pub const STATUS_LINE: &str = "status: hypothesized — regression lock, not box-verified";

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.hex"))
}

/// Hex text in 16-byte rows.
pub fn hex(b: &[u8]) -> String {
    let rows: Vec<String> = b
        .chunks(16)
        .map(|c| {
            c.iter()
                .map(|x| format!("{x:02x}"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    format!("{}\n", rows.join("\n"))
}

fn header_of(text: &str) -> String {
    text.lines()
        .take_while(|l| l.starts_with('#'))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// The vector's header lines (each starting with `#`) and its bytes.
pub fn load_with_header(name: &str) -> (String, Vec<u8>) {
    let p = path(name);
    let text = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("missing golden {}: {e}", p.display()));
    let bytes = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .flat_map(str::split_whitespace)
        .map(|h| u8::from_str_radix(h, 16).unwrap_or_else(|e| panic!("{name}: {h}: {e}")))
        .collect();
    (header_of(&text), bytes)
}

/// The vector's bytes; asserts the header carries the status line.
pub fn load(name: &str) -> Vec<u8> {
    let (header, bytes) = load_with_header(name);
    assert!(
        header.contains(STATUS_LINE),
        "golden {name}: header lacks `{STATUS_LINE}`"
    );
    bytes
}

/// Asserts `bytes` equal the vector `name` (or rewrites it under
/// `OOB_BLESS`).
pub fn check(name: &str, bytes: &[u8]) {
    if std::env::var_os("OOB_BLESS").is_some() {
        let header = std::fs::read_to_string(path(name))
            .map(|t| header_of(&t))
            .unwrap_or_else(|_| format!("# {STATUS_LINE}\n# TODO: record the hand cross-check\n"));
        std::fs::write(path(name), format!("{header}{}", hex(bytes))).unwrap();
        return;
    }
    let want = load(name);
    assert_eq!(
        bytes,
        want.as_slice(),
        "golden {name} differs; hex now:\n{}",
        hex(bytes)
    );
}
