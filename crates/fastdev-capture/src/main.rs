use anyhow::{Context, Result};
use clap::Parser;
use fastdev_common::{capture_live, CaptureOptions};
use std::fs;
use std::path::PathBuf;

/// Capture the live udev state of a reference boot. Run once
/// per board/kernel revision, on a system with the real systemd-udevd
/// present and settled.
#[derive(Parser)]
struct Args {
    #[arg(long)]
    board_id: String,

    #[arg(long)]
    out: PathBuf,

    #[arg(long, default_value = "/dev")]
    dev_root: PathBuf,

    #[arg(long, default_value = "/run/udev/data")]
    udev_data_root: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let opts = CaptureOptions {
        board_id: args.board_id,
        dev_root: args.dev_root,
        udev_data_root: args.udev_data_root,
    };

    let capture = capture_live(&opts)?;

    let json = serde_json::to_string_pretty(&capture).context("serializing capture")?;
    fs::write(&args.out, json).with_context(|| format!("writing {}", args.out.display()))?;

    eprintln!(
        "captured {} devices, {} /dev nodes -> {}",
        capture.devices.len(),
        capture.nodes.len(),
        args.out.display()
    );
    Ok(())
}
