use std::fs::OpenOptions;
use std::io::Write;

/// Best-effort info-level log to /dev/kmsg (KERN_INFO), for processes that
/// run before journald is guaranteed up: fastdev-check and
/// fastdev-prep run pre-boot, fastdev-gatekeeper runs from a udev rule.
/// Falls back to stderr if /dev/kmsg isn't writable (e.g. in tests, or
/// unprivileged runs) so logging never becomes a reason to fail.
pub fn info(tag: &str, message: &str) {
    write_kmsg(6, tag, message);
}

/// Same, at KERN_ERR priority.
pub fn error(tag: &str, message: &str) {
    write_kmsg(3, tag, message);
}

fn write_kmsg(level: u8, tag: &str, message: &str) {
    let line = format!("<{level}>{tag}: {message}\n");
    let wrote = OpenOptions::new()
        .write(true)
        .open("/dev/kmsg")
        .and_then(|mut f| f.write_all(line.as_bytes()))
        .is_ok();
    if !wrote {
        eprintln!("{tag}: {message}");
    }
}
