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
   checks keyed on physical port path, invoked from a real enforcement
   rule (`99-fastdev-usb.rules`) that's installed system-wide by
   `fastdev-usb.spec`. Because the stock udev chain is never masked, that
   rule is already live under the real daemon — no separate boot-time unit
   is needed for it to take effect. The earlier plan for a scoped
   fast-path `usb-udevd.service` belonged to the abandoned masking-based
   design and isn't needed now that udev stays in the loop; what's left of
   Milestone 5 is just an `udevadm test`-driven audit of which stock USB
   rule stanzas (mass-storage, serial, HID) should coexist with the
   gatekeeper rule.

An earlier "mask the stock udev chain, replay precomputed data directly"
design was built, extensively tested, and abandoned after real
`systemctl list-jobs` introspection showed it broke `.device` unit creation
(used by e.g. the ESP partition mount) — the ruleset-tweak approach above
exists because of what that investigation found.

## Installing the precomputed policy in an AIB manifest

The precomputed policy is just an RPM, `fastdev-data-<board_id>`, built from
a `fastdev-capture` + `fastdev-compile` run against a reference boot of the
real target hardware/board revision. There's one such RPM per `board_id`.

1. **Capture**, on a reference boot of the real board (needs a live, settled
   `systemd-udevd` — not something you can do as part of the image build
   itself):
   ```
   fastdev-capture --board-id <board_id> --out capture.json
   ```
   Pull `capture.json` off the device (`scp`, shared dir, etc.) onto the
   build host.

2. **Compile**, on the build host:
   ```
   fastdev-compile --in capture.json --board-id <board_id> \
       --out-dir fastdev-data-<board_id>/ \
       --include-subsystem block
   ```
   `--include-subsystem` is repeatable; scope it to the subsystems you
   actually want pre-seeded (typically just `block`, for the blkid-skip
   mechanism — run `fastdev-compile --help` for why including everything
   captured by default is usually the wrong call once GPU/optional-module
   devices are present).

3. **Package** the compiled output as an RPM with the template in
   `rpm/fastdev-data.spec.in`:
   ```
   rpmbuild -bb rpm/fastdev-data.spec.in \
       --define "board_id <board_id>" \
       --define "data_dir $PWD/fastdev-data-<board_id>"
   ```
   This produces `fastdev-data-<board_id>-<version>.<dist>.noarch.rpm`,
   containing the generated `10-fastdev-precomputed.rules` and the
   `check-manifest.json` `fastdev-check` validates live hardware against.
   Build `fastdev-prep`'s RPM too (it carries the `fastdev-check` binary),
   then put both into a local dnf repo:
   ```
   mkdir -p /path/to/repo && cp *.rpm /path/to/repo/ && createrepo_c /path/to/repo
   ```

4. **Reference it from the AIB manifest** (see `qemu/fastdev.aib.yml` for a
   complete worked example):
   ```yaml
   content:
     repos:
       - id: fastdev-local
         baseurl: file:///path/to/repo
         priority: 1
     rpms:
       - fastdev-prep
       - fastdev-data-<board_id>
     add_files:
       # Stand-in for the real provisioning/OTA system (open question) -
       # selects which board's data package fastdev-check validates
       # against at runtime.
       - path: /etc/fastdev-board-id
         text: "<board_id>"
   ```
   `fastdev-check` reads `/etc/fastdev-board-id` (override with the
   `FASTDEV_BOARD_ID_FILE` env var) to pick which
   `/usr/lib/fastdev/<board_id>/` data to validate against. The udev rule
   itself, installed unconditionally by `fastdev-data-<board_id>`, doesn't
   depend on that file — it matches on `DEVPATH` directly, so a missing or
   wrong board-id file only affects the diagnostic, never the speedup or
   its fail-safe fallback.

### Alternative: install the generated files directly, no RPM

The speed mechanism itself needs exactly one file —
`10-fastdev-precomputed.rules` is self-contained and matches on `DEVPATH`
directly, with no runtime dependency on `tmpfiles.conf` or the `udev-data/`
tree (those two are only consumed by the earlier, abandoned replay design,
`fastdev-prep` — see "How it works" above). `check-manifest.json` is needed
only if you also want the `fastdev-check` diagnostic. So for a single board
or quick local testing, skip steps 3-4 above and `add_files` the two
relevant outputs of step 2 straight into the manifest instead of building
an RPM — AIB's `add_files` supports a local `source_path`, not just inline
`text:`:
```yaml
content:
  add_files:
    - path: /usr/lib/udev/rules.d/10-fastdev-precomputed.rules
      source_path: fastdev-data-<board_id>/10-fastdev-precomputed.rules
    - path: /usr/lib/fastdev/<board_id>/check-manifest.json
      source_path: fastdev-data-<board_id>/check-manifest.json
    - path: /etc/fastdev-board-id
      text: "<board_id>"
  rpms:
    - fastdev-prep   # still needed for the fastdev-check binary itself
```
This is simpler for one-off boards, but you lose dnf-level
versioning/removal and the `fastdev-data.spec.in` template's actual design
intent — a separate package per board revision specifically so recapturing
a board's data never forces a `fastdev-prep` rebuild/requalification. Past
a handful of boards, or once you're regenerating captures regularly, the
RPM route pays for itself; for one board or a quick experiment, `add_files`
is less ceremony.

Recapture and rebuild `fastdev-data-<board_id>` whenever the board's storage
topology changes (new partition layout, different controller/bus
enumeration order, a kernel bump that changes device naming/order) — a
stale capture doesn't cause incorrect behavior (a mismatched device just
falls back to the real `blkid` probe, per device), it just means that
device gets no speedup until recaptured.

## Building the binary RPMs from a tarball

`fastdev-prep.spec`, `fastdev-usb.spec`, and `fastdev-tools.spec` have no
`Source0`/`%prep`/`%build` section - they expect release binaries to
already exist under `%{fastdev_src}/target/release/` (default
`%{_sourcedir}/..`, overridable with `--define "fastdev_src <path>"`) and
just copy them into the buildroot. To build from a tarball rather than a
live checkout:

1. Build the release binaries first - the specs copy them, they don't
   invoke `cargo`:
   ```
   cargo build --release --workspace
   ```

2. Tarball the source tree, including the now-built `target/release/`
   (`git archive` won't work here: `target/` is gitignored, and that's
   exactly what the specs read from):
   ```
   tar czf fastdev-0.1.0.tar.gz \
       --exclude=.git --exclude=build --exclude='target/debug' \
       --exclude='*.tar.gz' \
       -C .. "$(basename "$PWD")"
   ```

3. Set up a standard rpmbuild tree and extract the tarball into it:
   ```
   rpmdev-setuptree   # creates ~/rpmbuild/{SPECS,SOURCES,BUILD,RPMS,SRPMS}
   cp fastdev-0.1.0.tar.gz ~/rpmbuild/SOURCES/
   tar xzf ~/rpmbuild/SOURCES/fastdev-0.1.0.tar.gz -C ~/rpmbuild/SOURCES/
   ```

4. Build each package against the extracted tree:
   ```
   rpmbuild -bb rpm/fastdev-prep.spec \
       --define "fastdev_src $HOME/rpmbuild/SOURCES/fastdev-0.1.0"
   rpmbuild -bb rpm/fastdev-usb.spec \
       --define "fastdev_src $HOME/rpmbuild/SOURCES/fastdev-0.1.0"
   rpmbuild -bb rpm/fastdev-tools.spec \
       --define "fastdev_src $HOME/rpmbuild/SOURCES/fastdev-0.1.0"
   ```
   RPMs land in `~/rpmbuild/RPMS/<arch>/`. For a quick local build without
   a tarball, skip steps 2-3 and pass `--define "fastdev_src $PWD"` (the
   repo root) straight after step 1 instead.

## Status

The rule-based speed mechanism above is implemented and validated: boots
cleanly and reproducibly under QEMU (confirmed across multiple reboots),
with the measured `systemd-udev-trigger.service` improvement above.
`fastdev-prep`/`static-devprep.service` (the earlier, abandoned replay
mechanism) still exist as tested code but aren't wired into the boot path.
Milestone 5 (Phase 2 USB) has a working enforcement rule:
`fastdev-gatekeeper` and `99-fastdev-usb.rules` are implemented and
installed into the real `rules.d` by `fastdev-usb.spec`, so the
default-deny allow-list is already enforced under the stock udev chain
wherever that package is installed. The scoped `usb-udevd.service` design
is no longer needed — it was built for the abandoned masking-based
architecture, and the real udevd already handles USB hotplug unmodified.
What's left is just the `udevadm test`-driven audit of which stock USB
rule stanzas should coexist with the gatekeeper rule.

## Repository layout

```
crates/
  fastdev-common/       shared schema + live-capture logic
  fastdev-capture/      build-time: dump real udev state from a reference boot
  fastdev-compile/      build-time: capture.json -> 10-fastdev-precomputed.rules
                         (+ the earlier replay-mechanism artifacts, unused)
  fastdev-prep/         boot-time replay (built, tested, not wired in - see Status)
  fastdev-check/        boot-time diagnostic generator (validates, logs; no longer gates anything)
  fastdev-gatekeeper/   Phase 2 USB allow-list helper (implemented; invoked from
                        the enforcement rule below, already enforced under the
                        stock udev chain)
  fastdev-diff/         CI/validation tool, diffs live state against a capture
systemd/                static-devprep.service (earlier, abandoned replay design),
                        usb-udevd.service (earlier scoped-hotplug design,
                        superseded now that udev stays in the loop - see Status;
                        installed, not enabled)
udev-rules/             99-fastdev-usb.rules (Phase 2 USB gatekeeper enforcement
                        rule, implemented and live; installed by fastdev-usb.spec)
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
