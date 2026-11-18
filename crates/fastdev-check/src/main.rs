use fastdev_common::{kmsg, read_hw_identifier, CheckManifest, CAPTURE_SCHEMA_VERSION};
use std::fs;
use std::path::PathBuf;

const TAG: &str = "fastdev-check";

enum Outcome {
    Match,
    Mismatch(String),
}

/// Boot-time validation gate, installed as a systemd generator. This is
/// now purely diagnostic: it confirms the live hardware
/// matches what was captured and logs the result, but no longer touches
/// any systemd unit's enabled/masked state.
///
/// Earlier design masked the stock udev chain on a confirmed match, to
/// skip its real work. That approach was abandoned after extensive QEMU
/// testing: masking `systemd-udevd.service` - in any combination with
/// other units - reproduces an indefinite hang, because `.device` units
/// (which stock units like `boot-efi.mount` hardcode a dependency on) are
/// only ever created from real udev monitor broadcasts; masking the
/// daemon removes the only mechanism that ever satisfies them. Confirmed
/// directly via `systemctl list-jobs` during the hang: `dev-ttyS0.device`
/// and `dev-disk-by\x2dlabel-ESP.device` stuck in `running` forever.
///
/// The actual speed win now comes from `fastdev-compile`'s generated
/// `10-fastdev-precomputed.rules` (installed system-wide, sorted before
/// `60-persistent-storage.rules`): it pre-seeds each captured block
/// device's ID_FS_* properties and sets the stock, documented
/// `UDEV_DISABLE_PERSISTENT_STORAGE_BLKID_FLAG` escape hatch, so real
/// `systemd-udevd`/`systemd-udev-trigger.service`/`systemd-udev-settle.service`
/// all run completely unmodified - `.device` units keep working exactly as
/// on an unmodified system - while skipping blkid's real disk read (the
/// dominant cost) for devices already known to be safe. A device that
/// doesn't match any generated stanza (new/different hardware) falls
/// through to the real probe automatically, per-device, with no global
/// state to get wrong.
fn main() {
    // Still installed and invoked as a generator (argv[1] is the output
    // directory systemd expects generators to be callable with), even
    // though nothing is written there anymore - kept in case a future
    // milestone needs that capability again.
    if std::env::args().nth(1).is_none() {
        kmsg::error(TAG, "no generator output directory argument (not invoked as a systemd generator?)");
        return;
    }

    match check() {
        Ok(Outcome::Match) => kmsg::info(TAG, "snapshot validated against live hardware"),
        Ok(Outcome::Mismatch(reason)) => kmsg::info(TAG, &format!("mismatch ({reason})")),
        Err(err) => kmsg::error(TAG, &format!("check itself failed ({err:#})")),
    }
}

fn check() -> anyhow::Result<Outcome> {
    let board_id_file = env_path("FASTDEV_BOARD_ID_FILE", "/etc/fastdev-board-id");
    let base_dir = env_path("FASTDEV_BASE_DIR", "/usr/lib/fastdev");

    let board_id = match fs::read_to_string(&board_id_file) {
        Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => {
            return Ok(Outcome::Mismatch(format!(
                "{} missing or empty",
                board_id_file.display()
            )))
        }
    };

    let manifest_path = base_dir.join(&board_id).join("check-manifest.json");
    let manifest_text = match fs::read_to_string(&manifest_path) {
        Ok(t) => t,
        Err(_) => {
            return Ok(Outcome::Mismatch(format!(
                "no check-manifest at {}",
                manifest_path.display()
            )))
        }
    };
    let manifest: CheckManifest = match serde_json::from_str(&manifest_text) {
        Ok(m) => m,
        Err(_) => {
            return Ok(Outcome::Mismatch(format!(
                "unparseable check-manifest at {}",
                manifest_path.display()
            )))
        }
    };

    if manifest.schema_version != CAPTURE_SCHEMA_VERSION {
        return Ok(Outcome::Mismatch(format!(
            "schema_version {} != expected {}",
            manifest.schema_version, CAPTURE_SCHEMA_VERSION
        )));
    }

    let live_hw = read_hw_identifier();
    if live_hw != manifest.hw_identifier {
        return Ok(Outcome::Mismatch(format!(
            "hw_identifier mismatch: live='{live_hw}' expected='{}'",
            manifest.hw_identifier
        )));
    }

    // FASTDEV_SYS_ROOT lets tests point this at a fake tree instead of the
    // real /sys.
    let sys_override = std::env::var("FASTDEV_SYS_ROOT").ok();
    for syspath in &manifest.syspaths {
        let check_path = match &sys_override {
            Some(root) => {
                PathBuf::from(root).join(syspath.trim_start_matches('/').trim_start_matches("sys/"))
            }
            None => PathBuf::from(syspath),
        };
        if !check_path.exists() {
            return Ok(Outcome::Mismatch(format!("missing device: {syspath}")));
        }
    }

    Ok(Outcome::Match)
}

fn env_path(var: &str, default: &str) -> PathBuf {
    std::env::var(var).map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(default))
}
