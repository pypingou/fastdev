# fastdev

**fastdev speeds up automotive boot by pre-computing, at image build time,
the device metadata `udev` would otherwise have to re-probe from real disk
I/O on every single boot.** On a vehicle ECU, the storage layout doesn't
change between boots — so instead of letting `blkid` re-read every block
device's filesystem superblock at every boot, fastdev captures that
metadata *once* and ships it as a real udev rule that pre-seeds it,
skipping the actual disk read per device. The stock `udev` daemon, its
coldplug trigger, and its settle wait all keep running completely
unmodified — nothing is masked, disabled, or bypassed — so `.device` unit
creation, `libudev`/`sd-device` compatibility, and real USB hotplug all
keep working exactly as on an unmodified system. A device that doesn't
match the precomputed data (new/different hardware) silently falls back to
the real probe, automatically, per device — fastdev can only make boot
faster, never incorrect.

## Why

`systemd-udevd`'s boot-time cost — rule parsing, coldplug trigger, and the
synchronous disk I/O `blkid` does to probe filesystem metadata — is
measured at **~220ms** on a minimal automotive QEMU target
(`systemd-udev-trigger.service` 189ms + `systemd-udevd.service` 31ms).
Pre-seeding the properties `blkid` would have produced drops
`systemd-udev-trigger.service` to **~105-143ms** in testing, with zero
architectural risk, since nothing about the real udev chain is touched.

This is strictly a **post-switch-root** optimization: fastdev has never
targeted initrd-stage root-device discovery, which runs before any of this
and isn't udev-based in the first place (dracut today; the bootc/composefs
migration target, `autoinit`, explicitly does root lookup without udev or
libblkid at all — see its `src/block.rs`). There's no udev rule hook point
in that phase, so fastdev doesn't and can't touch it.

## How it works

1. **Build time**: boot a reference image once, capture the real udev
   database and `/dev` tree (`fastdev-capture`), and compile it
   (`fastdev-compile`) into a per-device udev rule
   (`10-fastdev-precomputed.rules`) that pre-seeds each captured block
   device's `ID_FS_*` properties and sets
   `UDEV_DISABLE_PERSISTENT_STORAGE_BLKID_FLAG` — a stock, documented
   escape hatch already present in `60-persistent-storage.rules` — for
   that one device.
2. **Boot time**: the rule is installed ahead of the stock storage rules
   by filename sort order. Real `systemd-udevd`, `systemd-udev-trigger.service`,
   and `systemd-udev-settle.service` run exactly as on an unmodified
   system; a device matching a generated stanza skips its real `blkid`
   probe, a device that doesn't (new/different hardware) falls through to
   the real probe automatically.
3. A separate diagnostic generator (`fastdev-check`) validates the live
   hardware against the capture at boot and logs the result via `kmsg` —
   informational only, it doesn't gate or control anything.
4. **USB hotplug** (`fastdev-gatekeeper`) is a default-deny VID:PID
   allow-list helper for the one genuinely dynamic bus, with anti-spoofing
   checks keyed on physical port path — not yet wired into a boot-time
   unit (Milestone 5, not started).

An earlier "mask the stock udev chain, replay precomputed data directly"
design was built, extensively tested, and abandoned after real
`systemctl list-jobs` introspection showed it broke `.device` unit creation
(used by e.g. the ESP partition mount) — the ruleset-tweak approach above
exists because of what that investigation found.

## Status

The rule-based speed mechanism above is implemented and validated: boots
cleanly and reproducibly under QEMU (confirmed across multiple reboots),
with the measured `systemd-udev-trigger.service` improvement above.
`fastdev-prep`/`static-devprep.service` (the earlier, abandoned replay
mechanism) still exist as tested code but aren't wired into the boot path.
Milestone 5 (Phase 2 USB) exists as scaffolding only —
`99-fastdev-usb.rules` is still placeholder content and nothing enables
`usb-udevd.service` yet.

## Repository layout

```
crates/
  fastdev-common/       shared schema + live-capture logic
  fastdev-capture/      build-time: dump real udev state from a reference boot
  fastdev-compile/      build-time: capture.json -> 10-fastdev-precomputed.rules
                         (+ the earlier replay-mechanism artifacts, unused)
  fastdev-prep/         boot-time replay (built, tested, not wired in - see Status)
  fastdev-check/        boot-time diagnostic generator (validates, logs; no longer gates anything)
  fastdev-gatekeeper/   Phase 2 USB allow-list helper (not yet wired in)
  fastdev-diff/         CI/validation tool, diffs live state against a capture
systemd/                static-devprep.service, usb-udevd.service (installed, not enabled)
udev-rules/             99-fastdev-usb.rules (Phase 2, placeholder content)
rpm/                    RPM spec files for all of the above
config/                 default usb-allowlist.toml
qemu/                   aib/aib-dev manifests for building test images
```

## Building and testing

```
cargo build --release --workspace   # build all binaries
cargo clippy --workspace            # lint
```

The QEMU test pipeline is capture → compile → package → `aib-dev build` →
`air` boot → `fastdev-diff` validate. Note: when booting manually for
debugging, avoid `air --nographics` combined
with non-interactive stdio (e.g. `nohup ... &`) — it was found to cause
console/stdio issues unrelated to fastdev itself. Prefer
`air -display none -serial unix:/path/to.sock,server,nowait`.
