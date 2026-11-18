use anyhow::{Context, Result};
use clap::Parser;
use fastdev_common::{capture_live, Capture, CaptureOptions, DevNode, DeviceRecord};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// CI shadow-mode validator: re-captures the running system
/// the same way fastdev-capture does and structurally diffs it against a
/// checked-in expected capture. Non-zero exit on any mismatch. Also usable
/// standalone for Spike 0's reboot-stability loop by diffing two captures
/// against each other.
#[derive(Parser)]
struct Args {
    #[arg(long)]
    expected: PathBuf,

    #[arg(long, default_value = "/dev")]
    actual_dev: PathBuf,

    #[arg(long, default_value = "/run/udev/data")]
    actual_db: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let expected_text = fs::read_to_string(&args.expected)
        .with_context(|| format!("reading {}", args.expected.display()))?;
    let expected: Capture = serde_json::from_str(&expected_text).context("parsing expected capture")?;

    let opts = CaptureOptions {
        board_id: expected.board_id.clone(),
        dev_root: args.actual_dev,
        udev_data_root: args.actual_db,
    };
    let actual = capture_live(&opts)?;

    let mismatches = diff(&expected, &actual);

    if mismatches.is_empty() {
        eprintln!("OK: no mismatches ({} devices, {} nodes)", expected.devices.len(), expected.nodes.len());
        return Ok(());
    }

    eprintln!("MISMATCH: {} difference(s) found:", mismatches.len());
    for m in &mismatches {
        eprintln!("  - {m}");
    }
    std::process::exit(1);
}

fn diff(expected: &Capture, actual: &Capture) -> Vec<String> {
    let mut mismatches = Vec::new();

    if expected.schema_version != actual.schema_version {
        mismatches.push(format!(
            "schema_version: expected {} got {}",
            expected.schema_version, actual.schema_version
        ));
    }
    if expected.hw_identifier != actual.hw_identifier {
        mismatches.push(format!(
            "hw_identifier: expected '{}' got '{}'",
            expected.hw_identifier, actual.hw_identifier
        ));
    }

    diff_devices(&expected.devices, &actual.devices, &mut mismatches);
    diff_nodes(&expected.nodes, &actual.nodes, &mut mismatches);

    mismatches
}

fn diff_devices(expected: &[DeviceRecord], actual: &[DeviceRecord], out: &mut Vec<String>) {
    let expected_by_id: BTreeMap<&str, &DeviceRecord> =
        expected.iter().map(|d| (d.db_id.as_str(), d)).collect();
    let actual_by_id: BTreeMap<&str, &DeviceRecord> =
        actual.iter().map(|d| (d.db_id.as_str(), d)).collect();

    for (id, dev) in &expected_by_id {
        match actual_by_id.get(id) {
            None => out.push(format!("device {id} ({}) missing in actual", dev.syspath)),
            Some(actual_dev) => {
                if dev.db_text != actual_dev.db_text {
                    out.push(format!("device {id}: db_text differs"));
                }
            }
        }
    }
    for id in actual_by_id.keys() {
        if !expected_by_id.contains_key(id) {
            out.push(format!("device {id} present in actual but not expected"));
        }
    }
}

fn diff_nodes(expected: &[DevNode], actual: &[DevNode], out: &mut Vec<String>) {
    let expected_by_path: BTreeMap<&str, &DevNode> =
        expected.iter().map(|n| (n.path.as_str(), n)).collect();
    let actual_by_path: BTreeMap<&str, &DevNode> =
        actual.iter().map(|n| (n.path.as_str(), n)).collect();

    for (path, node) in &expected_by_path {
        match actual_by_path.get(path) {
            None => out.push(format!("node {path} missing in actual")),
            Some(actual_node) => {
                if node.mode != actual_node.mode || node.uid != actual_node.uid || node.gid != actual_node.gid {
                    out.push(format!(
                        "node {path}: expected mode={:04o} uid={} gid={} got mode={:04o} uid={} gid={}",
                        node.mode, node.uid, node.gid, actual_node.mode, actual_node.uid, actual_node.gid
                    ));
                }
            }
        }
    }
    for path in actual_by_path.keys() {
        if !expected_by_path.contains_key(path) {
            out.push(format!("node {path} present in actual but not expected"));
        }
    }
}
