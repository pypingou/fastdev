use fastdev_common::kmsg;
use serde::Deserialize;
use std::path::{Path, PathBuf};

const TAG: &str = "fastdev-gatekeeper";

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
enum Decision {
    Allow,
    Deny,
}

impl Decision {
    fn as_allow_flag(self) -> &'static str {
        match self {
            Decision::Allow => "1",
            Decision::Deny => "0",
        }
    }
}

#[derive(Deserialize, Clone)]
struct IdEntry {
    vendor_id: String,
    model_id: String,
}

#[derive(Deserialize)]
struct AllowlistConfig {
    #[serde(default = "deny_default")]
    default: Decision,
    #[serde(default)]
    entries: Vec<IdEntry>,
    /// VID:PID used by internal/fixed USB peripherals from the Phase 1
    /// capture - trusted only when also arriving on a trusted port path:
    /// a device claiming one of these IDs on any other port is a spoofing
    /// attempt, denied unconditionally.
    #[serde(default)]
    reserved: Vec<IdEntry>,
    #[serde(default)]
    trusted_port_prefixes: Vec<String>,
}

fn deny_default() -> Decision {
    Decision::Deny
}

/// Invoked from a udev rule as `IMPORT{program}="... check"`.
/// Default-deny throughout: a missing/unparseable config, a
/// missing env var, or any internal error all resolve to FASTDEV_ALLOW=0,
/// never to silent admission.
fn main() {
    let vendor_id = std::env::var("ID_VENDOR_ID").unwrap_or_default();
    let model_id = std::env::var("ID_MODEL_ID").unwrap_or_default();
    let devpath = std::env::var("DEVPATH").unwrap_or_default();

    let config_path = std::env::var("FASTDEV_ALLOWLIST_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/etc/fastdev/usb-allowlist.toml"));

    let decision = decide(&config_path, &vendor_id, &model_id, &devpath);

    println!("FASTDEV_ALLOW={}", decision.as_allow_flag());
    kmsg::info(
        TAG,
        &format!("{vendor_id}:{model_id} at {devpath} -> {decision:?}"),
    );
}

fn decide(config_path: &Path, vendor_id: &str, model_id: &str, devpath: &str) -> Decision {
    let config = match load_config(config_path) {
        Ok(c) => c,
        Err(err) => {
            kmsg::error(TAG, &format!("config load failed ({err}), denying"));
            return Decision::Deny;
        }
    };

    let matches = |entry: &IdEntry| entry.vendor_id == vendor_id && entry.model_id == model_id;

    if config.reserved.iter().any(matches) {
        let on_trusted_port = config
            .trusted_port_prefixes
            .iter()
            .any(|prefix| devpath.starts_with(prefix.as_str()));
        if on_trusted_port {
            return Decision::Allow;
        }
        kmsg::error(
            TAG,
            &format!("{vendor_id}:{model_id} claims a reserved ID but arrived at {devpath}, not a trusted port - denying (possible spoof)"),
        );
        return Decision::Deny;
    }

    if config.entries.iter().any(matches) {
        return Decision::Allow;
    }

    config.default
}

fn load_config(path: &Path) -> anyhow::Result<AllowlistConfig> {
    let text = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}
