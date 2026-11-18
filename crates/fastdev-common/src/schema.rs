use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Bump on any incompatible field change. `fastdev-compile`/`fastdev-check`
/// must refuse to load a mismatched version rather than silently misparse it.
pub const CAPTURE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capture {
    pub schema_version: u32,
    pub board_id: String,
    /// DT `model` or DMI string, for fastdev-check's cheap cross-check.
    pub hw_identifier: String,
    pub kernel_release: String,
    /// Informational only (decimal seconds since the Unix epoch).
    pub captured_at: String,
    pub devices: Vec<DeviceRecord>,
    pub nodes: Vec<DevNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceRecord {
    /// Full /sys path, e.g. "/sys/devices/pci0000:00/.../block/sda" — stable key.
    pub syspath: String,
    /// Kernel DEVPATH (syspath relative to /sys).
    pub devpath: Option<String>,
    pub subsystem: String,
    pub properties: BTreeMap<String, String>,
    pub tags: Vec<String>,
    /// udevd's db key: b<maj>:<min>, c<maj>:<min>, n<ifindex>, or +subsystem:sysname.
    pub db_id: String,
    /// Verbatim bytes of /run/udev/data/<db_id>, copied rather than
    /// reconstructed to avoid drift from whatever internal format
    /// systemd-udevd uses for the pinned version.
    pub db_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevNode {
    /// e.g. "/dev/ttyLIN0".
    pub path: String,
    pub kind: NodeKind,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeKind {
    CharDevice { major: u32, minor: u32 },
    BlockDevice { major: u32, minor: u32 },
    Symlink { target: String },
    Directory,
}

/// Small subset of a Capture kept separate from the full udev-data tree so
/// fastdev-check only has to read a few KB at boot, not the full per-device
/// db text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckManifest {
    pub schema_version: u32,
    pub board_id: String,
    pub hw_identifier: String,
    pub syspaths: Vec<String>,
}

impl CheckManifest {
    pub fn from_capture(capture: &Capture) -> Self {
        Self::from_capture_filtered(capture, &[])
    }

    /// `subsystems` empty means "no filtering, include every captured
    /// device". Non-empty restricts the existence check to devices on
    /// those subsystems only.
    ///
    /// This matters beyond convenience: a systemd generator runs strictly
    /// before systemd starts *any* unit, including whatever
    /// `modprobe@<module>.service` would load a not-yet-loaded kernel
    /// module. A device whose driver is loaded that way (common for
    /// optional/hardware-detected modules - discovered the hard way with
    /// a QEMU `bochs-drm` display adapter, whose module load didn't even
    /// start until ~50ms after the generator had already run and declared
    /// it "missing") can never be present at generator-time, no matter how
    /// long a bounded retry waits. Scoping the check to the genuinely
    /// fixed/boot-critical subsystems (the original scope: storage,
    /// network, CAN - not every device udev happens to create a db entry
    /// for) avoids false mismatches from devices the check was never meant
    /// to gate on in the first place.
    pub fn from_capture_filtered(capture: &Capture, subsystems: &[String]) -> Self {
        let include = |d: &&DeviceRecord| subsystems.is_empty() || subsystems.contains(&d.subsystem);
        CheckManifest {
            schema_version: capture.schema_version,
            board_id: capture.board_id.clone(),
            hw_identifier: capture.hw_identifier.clone(),
            syspaths: capture.devices.iter().filter(include).map(|d| d.syspath.clone()).collect(),
        }
    }
}

/// Computes udevd's `/run/udev/data/<id>` key from a device's properties,
/// following systemd's device_get_id() scheme: network devices key off
/// ifindex, devices with a devnum key off major:minor (block vs char), and
/// everything else falls back to "+subsystem:sysname".
///
/// NOTE: this scheme is internal/unstable by design (an open risk) —
/// validate derived ids against real
/// `/run/udev/data` filenames for the pinned systemd version during
/// Milestone 1. `capture_live` already surfaces a mismatch loudly (a
/// missing db file for a derived id), so a wrong guess here fails visibly
/// rather than silently.
pub fn compute_db_id(subsystem: &str, devpath: &str, properties: &BTreeMap<String, String>) -> String {
    if let Some(ifindex) = properties.get("IFINDEX") {
        return format!("n{ifindex}");
    }
    if let (Some(major), Some(minor)) = (properties.get("MAJOR"), properties.get("MINOR")) {
        let kind = if subsystem == "block" { 'b' } else { 'c' };
        return format!("{kind}{major}:{minor}");
    }
    let sysname = devpath.rsplit('/').next().unwrap_or(devpath);
    format!("+{subsystem}:{sysname}")
}
