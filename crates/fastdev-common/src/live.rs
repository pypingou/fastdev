use crate::schema::{compute_db_id, Capture, DevNode, DeviceRecord, NodeKind, CAPTURE_SCHEMA_VERSION};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Where to read live system state from. Parameterized (rather than
/// hardcoding "/dev" and "/run/udev/data") so this can be exercised in
/// tests/CI without needing real hardware or root.
#[derive(Debug, Clone)]
pub struct CaptureOptions {
    pub board_id: String,
    pub dev_root: PathBuf,
    pub udev_data_root: PathBuf,
}

impl CaptureOptions {
    pub fn new(board_id: impl Into<String>) -> Self {
        CaptureOptions {
            board_id: board_id.into(),
            dev_root: PathBuf::from("/dev"),
            udev_data_root: PathBuf::from("/run/udev/data"),
        }
    }
}

/// Shared by `fastdev-capture` (writes the result to a file) and
/// `fastdev-diff` (compares it in-memory against an expected capture) —
/// both need to "capture the same way", so the logic lives once, here.
pub fn capture_live(opts: &CaptureOptions) -> Result<Capture> {
    let devices = capture_devices(&opts.udev_data_root)?;
    let nodes = capture_dev_nodes(&opts.dev_root)?;
    let hw_identifier = read_hw_identifier();
    let kernel_release = read_kernel_release()?;
    let captured_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();

    Ok(Capture {
        schema_version: CAPTURE_SCHEMA_VERSION,
        board_id: opts.board_id.clone(),
        hw_identifier,
        kernel_release,
        captured_at,
        devices,
        nodes,
    })
}

struct RawDevice {
    devpath: String,
    properties: BTreeMap<String, String>,
}

fn capture_devices(udev_data_root: &Path) -> Result<Vec<DeviceRecord>> {
    let output = Command::new("udevadm")
        .args(["info", "--export-db"])
        .output()
        .context("running `udevadm info --export-db`")?;
    anyhow::ensure!(
        output.status.success(),
        "udevadm info --export-db exited with {}",
        output.status
    );
    let text = String::from_utf8(output.stdout).context("udevadm output was not valid UTF-8")?;

    let mut devices = Vec::new();
    let mut missing_db_files = 0usize;
    for raw in parse_export_db(&text) {
        let subsystem = raw.properties.get("SUBSYSTEM").cloned().unwrap_or_default();
        let syspath = format!("/sys{}", raw.devpath);
        let tags = raw
            .properties
            .get("TAGS")
            .map(|t| t.split(':').filter(|s| !s.is_empty()).map(String::from).collect())
            .unwrap_or_default();
        let db_id = compute_db_id(&subsystem, &raw.devpath, &raw.properties);

        let db_text = match fs::read_to_string(udev_data_root.join(&db_id)) {
            Ok(text) => text,
            Err(_) => {
                missing_db_files += 1;
                eprintln!(
                    "warning: no {}/{db_id} for device {syspath} (derived db_id may be wrong)",
                    udev_data_root.display()
                );
                String::new()
            }
        };

        devices.push(DeviceRecord {
            syspath,
            devpath: Some(raw.devpath),
            subsystem,
            properties: raw.properties,
            tags,
            db_id,
            db_text,
        });
    }

    if missing_db_files > 0 {
        eprintln!(
            "warning: {missing_db_files}/{} devices had no matching udev db file",
            devices.len()
        );
    }

    Ok(devices)
}

fn parse_export_db(text: &str) -> Vec<RawDevice> {
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        if block.trim().is_empty() {
            continue;
        }
        let mut devpath = None;
        let mut properties = BTreeMap::new();
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("P: ") {
                devpath = Some(rest.to_string());
            } else if let Some(rest) = line.strip_prefix("E: ") {
                if let Some((k, v)) = rest.split_once('=') {
                    properties.insert(k.to_string(), v.to_string());
                }
            }
            // N:/S:/W: lines deliberately ignored here — device node and
            // symlink state comes from the /dev walk instead, so we don't
            // need to parse them twice.
        }
        if let Some(devpath) = devpath {
            out.push(RawDevice { devpath, properties });
        }
    }
    out
}

fn capture_dev_nodes(dev_root: &Path) -> Result<Vec<DevNode>> {
    let mut nodes = Vec::new();
    for entry in walkdir::WalkDir::new(dev_root).into_iter() {
        let entry = entry.context("walking /dev")?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(path)
            .with_context(|| format!("stat {}", path.display()))?;
        let file_type = metadata.file_type();

        let kind = if file_type.is_symlink() {
            let target = fs::read_link(path)
                .with_context(|| format!("readlink {}", path.display()))?;
            NodeKind::Symlink { target: target.to_string_lossy().into_owned() }
        } else if file_type.is_char_device() {
            let rdev = metadata.rdev();
            NodeKind::CharDevice { major: nix::sys::stat::major(rdev) as u32, minor: nix::sys::stat::minor(rdev) as u32 }
        } else if file_type.is_block_device() {
            let rdev = metadata.rdev();
            NodeKind::BlockDevice { major: nix::sys::stat::major(rdev) as u32, minor: nix::sys::stat::minor(rdev) as u32 }
        } else if file_type.is_dir() {
            NodeKind::Directory
        } else {
            // Regular files, sockets, fifos aren't expected under /dev;
            // skip rather than guess at how to replay them.
            continue;
        };

        nodes.push(DevNode {
            path: path.to_string_lossy().into_owned(),
            kind,
            mode: metadata.mode() & 0o7777,
            uid: metadata.uid(),
            gid: metadata.gid(),
        });
    }
    Ok(nodes)
}

/// DT `model` first (automotive/embedded ARM), falling back to a DMI string
/// (x86 boards, and the QEMU x86_64 dev target). Empty string if neither is
/// readable — fastdev-check treats that as a mismatch, same as any other
/// validation failure.
pub fn read_hw_identifier() -> String {
    if let Ok(s) = fs::read_to_string("/sys/firmware/devicetree/base/model") {
        let trimmed = s.trim_end_matches('\0').trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Ok(s) = fs::read_to_string("/sys/class/dmi/id/product_name") {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    String::new()
}

fn read_kernel_release() -> Result<String> {
    let uts = nix::sys::utsname::uname().context("uname()")?;
    Ok(uts.release().to_string_lossy().into_owned())
}
