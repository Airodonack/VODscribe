// Runs the end-to-end tests on Linux distributions, each in a container:
// our package installed with the distribution's own package manager (so its
// dependencies resolve from that distribution), then the suite, headless
// (Xvfb, with openbox as the window manager), through WebDriver.
//
//   node tests/e2e/distros.mjs <folder with the .deb and .rpm> [--only a,b] [-- run.mjs options]
//
// Arch-based ones get no package of ours yet: the .deb's files are unpacked
// over Arch's own libraries, as an AUR package built from it would, to see
// what such users would meet.
//
// Containers can't show desktop behaviour (window managers, portals,
// Wayland): only that the app installs, starts and works on each one's
// libraries.
//
// Environment:
//   E2E_PODMAN   the podman command (default "podman"; a wrapper for another store)
//   E2E_SEED     a data folder with bin/ and models/ to start each run with
//                (default: none, so first-run setup downloads them)
//   E2E_WORK     scratch space: each run's data folder (default tests/e2e/out/distros)
// Results: tests/e2e/out/distros/<name>/ (results.json, run.log, screenshots).

import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.join(here, "..", "..");

const APT = (image) => ({
  image,
  kind: "deb",
  // The WebDriver package was renamed in Ubuntu 26.04 (webkitgtk-webdriver).
  install:
    `export DEBIAN_FRONTEND=noninteractive; apt-get update -qq && ` +
    `driver=$(apt-cache policy webkit2gtk-driver | grep -q "Candidate: [0-9]" && echo webkit2gtk-driver || echo webkitgtk-webdriver) && ` +
    `apt-get install -y -qq /pkg/app.deb $driver xvfb xauth openbox xdotool dbus procps >/dev/null`,
});
const DNF = (image) => ({
  image,
  kind: "rpm",
  install: `dnf install -y -q /pkg/app.rpm webkitgtk6.0 xorg-x11-server-Xvfb xorg-x11-xauth openbox xdotool dbus-daemon procps-ng >/dev/null`,
});
// Arch: the libraries the .deb's binaries link, from Arch, and the .deb unpacked over /.
const PACMAN = (image) => ({
  image,
  kind: "deb",
  install:
    // What an Arch package of ours would have to depend on beyond WebKitGTK: alsa-lib (the app
    // links ALSA for the player's sound), and GStreamer's plugins, optional for WebKitGTK there,
    // without which its web process aborts at the first sound ("autoaudiosink not found").
    `pacman -Syu --noconfirm --needed webkit2gtk-4.1 webkitgtk-6.0 gtk3 libsoup3 alsa-lib gst-plugins-base gst-plugins-good gst-plugins-bad gst-libav ` +
    `xorg-server-xvfb xorg-xauth openbox xdotool dbus libarchive procps-ng >/dev/null && ` +
    `mkdir -p /tmp/deb && cd /tmp/deb && bsdtar -xf /pkg/app.deb && tar -xf data.tar.* -C / && cd / && ` +
    // pacman's hooks can't all run in a container: GTK's settings schemas, compiled by one, by hand.
    `glib-compile-schemas /usr/share/glib-2.0/schemas`,
});

/** Name: how to install there. Images as their registries name them. */
export const DISTROS = {
  "ubuntu-22.04": APT("docker.io/library/ubuntu:22.04"),
  "ubuntu-24.04": APT("docker.io/library/ubuntu:24.04"),
  "ubuntu-26.04": APT("docker.io/library/ubuntu:26.04"),
  "mint-22.3": APT("docker.io/linuxmintd/mint22.3-amd64"),
  "debian-12": APT("docker.io/library/debian:12"),
  "debian-13": APT("docker.io/library/debian:13"),
  "fedora-43": DNF("registry.fedoraproject.org/fedora:43"),
  "fedora-44": DNF("registry.fedoraproject.org/fedora:44"),
  arch: PACMAN("docker.io/library/archlinux:latest"),
  cachyos: PACMAN("docker.io/cachyos/cachyos:latest"),
};

const { values: opts, positionals } = parseArgs({
  allowPositionals: true,
  options: { only: { type: "string" } },
});
const [packages, ...rest] = positionals;
if (!packages) {
  console.error("usage: node tests/e2e/distros.mjs <folder with the .deb and .rpm> [--only a,b] [-- run.mjs options]");
  process.exit(2);
}
const find = (ext) =>
  readdirSync(packages, { recursive: true })
    .map(String)
    .find((f) => f.endsWith(ext));
const pkgs = { deb: find(".deb"), rpm: find(".rpm") };
const podman = (process.env.E2E_PODMAN ?? "podman").split(" ");
const which = (cmd) => spawnSync("sh", ["-c", `command -v ${cmd}`], { encoding: "utf8" }).stdout.trim();
// The test tools, from this computer, into each container (read-only).
const node = path.dirname(path.dirname(process.execPath));
const tauriDriver = process.env.TAURI_DRIVER ?? which("tauri-driver");
if (!tauriDriver) throw new Error("no tauri-driver: put it on PATH or set TAURI_DRIVER");
const work = process.env.E2E_WORK ?? path.join(here, "out", "distros");
const names = opts.only?.split(",") ?? Object.keys(DISTROS);

const summary = [];
for (const name of names) {
  const distro = DISTROS[name];
  if (!distro) throw new Error(`no distro ${name} (there are: ${Object.keys(DISTROS).join(", ")})`);
  const pkg = pkgs[distro.kind];
  if (!pkg) throw new Error(`no .${distro.kind} in ${packages}`);
  const out = path.join(here, "out", "distros", name);
  const data = path.join(work, `${name}-data`, "app.vodscribe");
  rmSync(out, { recursive: true, force: true });
  rmSync(path.dirname(data), { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  mkdirSync(data, { recursive: true });
  for (const kept of ["bin", "models"]) {
    const from = process.env.E2E_SEED && path.join(process.env.E2E_SEED, kept);
    if (from && existsSync(from)) cpSync(from, path.join(data, kept), { recursive: true });
  }
  const args = rest.map((a) => `'${a.replaceAll("'", "'\\''")}'`).join(" ");
  const script = [
    "set -e",
    distro.install,
    "export XDG_DATA_HOME=/data E2E_RESET_DATA=1 VODSCRIBE_NO_UPDATE_CHECK=1 E2E_OUT=/out TAURI_DRIVER=/opt/tools/tauri-driver",
    `echo "installed: $(. /etc/os-release; echo $PRETTY_NAME)"`,
    // An app that can't load its libraries never starts: say which, rather than time out each scenario.
    `missing=$(ldd /usr/bin/vodscribe /usr/bin/vodscribe-worker | grep "not found" || true); if [ -n "$missing" ]; then echo "missing libraries: $missing" | tee /out/run.log; exit 3; fi`,
    `cd /repo && dbus-run-session -- xvfb-run -a -s "-screen 0 1400x900x24" sh -c 'openbox & sleep 1; exec /opt/node/bin/node tests/e2e/run.mjs /usr/bin/vodscribe ${args.replaceAll("'", "'\\''")}' > /out/run.log 2>&1`,
  ].join("\n");
  console.log(`== ${name} (${distro.image}, .${distro.kind})`);
  const run = spawnSync(
    podman[0],
    [
      ...podman.slice(1),
      "run",
      "--rm",
      "-v",
      `${repo}:/repo:ro`,
      "-v",
      `${path.resolve(packages, pkg)}:/pkg/app.${distro.kind}:ro`,
      "-v",
      `${node}:/opt/node:ro`,
      "-v",
      `${tauriDriver}:/opt/tools/tauri-driver:ro`,
      "-v",
      `${path.dirname(data)}:/data`,
      "-v",
      `${out}:/out`,
      distro.image,
      "bash",
      "-c",
      script,
    ],
    { stdio: ["ignore", "inherit", "inherit"] },
  );
  const results = existsSync(path.join(out, "results.json"))
    ? JSON.parse(readFileSync(path.join(out, "results.json"), "utf8"))
    : [];
  const failed = results.filter((r) => !r.ok);
  const line =
    results.length === 0
      ? `no results (exit ${run.status}): see ${path.join(out, "run.log")}`
      : `${results.length - failed.length} of ${results.length} passed${failed.length ? `; failed: ${failed.map((r) => r.name).join(", ")}` : ""}`;
  console.log(`   ${line}`);
  summary.push(`${name}: ${line}`);
  rmSync(path.dirname(data), { recursive: true, force: true });
}
console.log(`\n${summary.join("\n")}`);
