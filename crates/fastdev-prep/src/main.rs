use anyhow::{bail, Context, Result};
use fastdev_common::kmsg;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const TAG: &str = "fastdev-prep";

/// Phase 1 boot-time replay. No CLI args — reads
/// /etc/fastdev-board-id (overridable via env vars below for testing
/// without root/real paths) and replays that board's precomputed tmpfiles +
/// udev database into place.
struct Paths {
    board_id_file: PathBuf,
    base_dir: PathBuf,
    udev_data_dir: PathBuf,
}

impl Paths {
    fn from_env() -> Self {
        Paths {
            board_id_file: env_path("FASTDEV_BOARD_ID_FILE", "/etc/fastdev-board-id"),
            base_dir: env_path("FASTDEV_BASE_DIR", "/usr/lib/fastdev"),
            udev_data_dir: env_path("FASTDEV_UDEV_DATA_DIR", "/run/udev/data"),
        }
    }
}

fn env_path(var: &str, default: &str) -> PathBuf {
    std::env::var(var).map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(default))
}

fn main() {
    if let Err(err) = run() {
        kmsg::error(TAG, &format!("failed: {err:#}"));
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let paths = Paths::from_env();

    let board_id = fs::read_to_string(&paths.board_id_file)
        .with_context(|| format!("reading {}", paths.board_id_file.display()))?
        .trim()
        .to_string();
    if board_id.is_empty() {
        bail!("{} is empty", paths.board_id_file.display());
    }

    let board_dir = paths.base_dir.join(&board_id);

    let tmpfiles_conf = board_dir.join("tmpfiles.conf");
    run_tmpfiles_create(&tmpfiles_conf)?;

    let udev_data_src = board_dir.join("udev-data");
    let copied = replay_udev_data(&udev_data_src, &paths.udev_data_dir)?;

    kmsg::info(
        TAG,
        &format!("replayed board '{board_id}': tmpfiles from {tmpfiles_conf:?}, {copied} udev db entries"),
    );
    Ok(())
}

fn run_tmpfiles_create(tmpfiles_conf: &Path) -> Result<()> {
    let status = Command::new("systemd-tmpfiles")
        .arg("--create")
        .arg(tmpfiles_conf)
        .status()
        .context("spawning systemd-tmpfiles")?;
    if !status.success() {
        bail!("systemd-tmpfiles --create {tmpfiles_conf:?} exited with {status}");
    }
    Ok(())
}

/// Atomic per-file staging: write to a temp name in the
/// destination directory, then rename — same-filesystem rename is atomic,
/// so a crash mid-run leaves each device's db entry either fully written or
/// not written at all, never partial/corrupt.
fn replay_udev_data(src_dir: &Path, dest_dir: &Path) -> Result<usize> {
    if !src_dir.is_dir() {
        kmsg::info(
            TAG,
            &format!("no {} directory - nothing to replay for this board", src_dir.display()),
        );
        return Ok(0);
    }

    fs::create_dir_all(dest_dir).with_context(|| format!("creating {}", dest_dir.display()))?;

    let mut count = 0;
    for entry in fs::read_dir(src_dir).with_context(|| format!("reading {}", src_dir.display()))? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let contents = fs::read(entry.path())
            .with_context(|| format!("reading {}", entry.path().display()))?;

        let tmp_name = format!(".fastdev-{}.tmp", name.to_string_lossy());
        let tmp_path = dest_dir.join(&tmp_name);
        let final_path = dest_dir.join(&name);

        fs::write(&tmp_path, &contents)
            .with_context(|| format!("writing {}", tmp_path.display()))?;
        fs::rename(&tmp_path, &final_path)
            .with_context(|| format!("renaming {} -> {}", tmp_path.display(), final_path.display()))?;
        count += 1;
    }
    Ok(count)
}
