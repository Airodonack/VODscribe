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
// By default, under X11 (Xvfb with openbox) in the light scheme: that the
// app installs, starts and works on each one's libraries. The options run
// it in other display environments, for how it behaves there (both of the
// embedded player's bugs seen on a desktop hid from the default):
//
//   --env weston|gnome   headless weston, or headless GNOME Shell (its own
//                        D-Bus and settings; its screenshots allowed by a
//                        test extension, gnome-shell/), Wayland both
//   --scheme dark        the dark colour scheme (GTK's theme, GNOME's setting)
//   --gpu nvidia         the host's NVIDIA GPU in the container (podman's
//                        CDI device), drawing with NVIDIA's own libraries
//   --gpu dri            the host's /dev/dri in the container, drawing with
//                        the distribution's Mesa (Intel and AMD GPUs; on a
//                        GPU Mesa has no driver for, its software fallback)
//
// Portals, real desktops and window managers' own ways aren't here: that's
// the QA VMs'.
//
// Behaviour on a few distributions, every environment, e.g.:
//   node tests/e2e/distros.mjs pkgs --only debian-13,arch --env gnome --scheme dark -- --only site-embed
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

/** What each display environment needs installed, by package manager. */
const ENV_PACKAGES = {
  x11: { apt: [], dnf: [], pacman: [] },
  weston: { apt: ["weston"], dnf: ["weston"], pacman: ["weston"] },
  gnome: { apt: ["gnome-shell", "libglib2.0-bin"], dnf: ["gnome-shell"], pacman: ["gnome-shell"] },
};

/** What each GPU needs installed, by package manager: NVIDIA's libraries come with its CDI device. */
const GPU_PACKAGES = {
  nvidia: { apt: [], dnf: [], pacman: [] },
  dri: {
    apt: ["libgl1-mesa-dri", "libegl-mesa0", "libgbm1"],
    dnf: ["mesa-dri-drivers", "mesa-libEGL", "mesa-libgbm"],
    pacman: ["mesa"],
  },
};
/** How each GPU gets into the container (podman's options). */
const GPU_DEVICES = {
  nvidia: ["--device", "nvidia.com/gpu=all"],
  // The render nodes belong to the video and render groups: rootless podman
  // drops the user's groups unless asked to keep them (crun only).
  dri: ["--device", "/dev/dri", "--group-add", "keep-groups"],
};

const APT = (image) => ({
  image,
  kind: "deb",
  pm: "apt",
  // The WebDriver package was renamed in Ubuntu 26.04 (webkitgtk-webdriver).
  install:
    `export DEBIAN_FRONTEND=noninteractive; apt-get update -qq && ` +
    `driver=$(apt-cache policy webkit2gtk-driver | grep -q "Candidate: [0-9]" && echo webkit2gtk-driver || echo webkitgtk-webdriver) && ` +
    `apt-get install -y -qq /pkg/app.deb $driver xvfb xauth openbox xdotool dbus procps $EXTRA >/dev/null`,
});
const DNF = (image) => ({
  image,
  kind: "rpm",
  pm: "dnf",
  install: `dnf install -y -q /pkg/app.rpm webkitgtk6.0 xorg-x11-server-Xvfb xorg-x11-xauth openbox xdotool dbus-daemon procps-ng $EXTRA >/dev/null`,
});
// Arch: the libraries the .deb's binaries link, from Arch, and the .deb unpacked over /.
const PACMAN = (image) => ({
  image,
  kind: "deb",
  pm: "pacman",
  install:
    // What an Arch package of ours would have to depend on beyond WebKitGTK: alsa-lib (the app
    // links ALSA for the player's sound), and GStreamer's plugins, optional for WebKitGTK there,
    // without which its web process aborts at the first sound ("autoaudiosink not found").
    `pacman -Syu --noconfirm --needed webkit2gtk-4.1 webkitgtk-6.0 gtk3 libsoup3 alsa-lib gst-plugins-base gst-plugins-good gst-plugins-bad gst-libav glycin bubblewrap ` +
    `xorg-server-xvfb xorg-xauth openbox xdotool dbus libarchive procps-ng $EXTRA >/dev/null && ` +
    `mkdir -p /tmp/deb && cd /tmp/deb && bsdtar -xf /pkg/app.deb && tar -xf data.tar.* -C / && cd / && ` +
    // pacman's hooks can't all run in a container: GTK's settings schemas by hand.
    `glib-compile-schemas /usr/share/glib-2.0/schemas`,
  // Images decode through glycin's loaders, each in a bubblewrap sandbox, as
  // on a desktop; bubblewrap makes namespaces, which rootless podman allows
  // only a privileged container (still with no more than the user's rights).
  podman: ["--privileged"],
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
  options: {
    only: { type: "string" },
    env: { type: "string", default: "x11" },
    scheme: { type: "string", default: "light" },
    gpu: { type: "string" },
  },
});
const [packages, ...rest] = positionals;
if (!packages) {
  console.error(
    "usage: node tests/e2e/distros.mjs <folder with the .deb and .rpm> [--only a,b] [--env x11|weston|gnome] [--scheme light|dark] [--gpu nvidia|dri] [-- run.mjs options]",
  );
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
if (!Object.hasOwn(ENV_PACKAGES, opts.env))
  throw new Error(`no environment ${opts.env} (there are: ${Object.keys(ENV_PACKAGES).join(", ")})`);
if (!["light", "dark"].includes(opts.scheme)) throw new Error(`no scheme ${opts.scheme} (light, dark)`);
if (opts.gpu && !Object.hasOwn(GPU_PACKAGES, opts.gpu))
  throw new Error(`no GPU ${opts.gpu} (there are: ${Object.keys(GPU_PACKAGES).join(", ")})`);
/** The cell's name: the distribution, then what differs from the default. */
const cell = (name) =>
  [name, opts.env !== "x11" && opts.env, opts.scheme !== "light" && opts.scheme, opts.gpu].filter(Boolean).join("-");

/** Starts the display environment and runs `run` (a shell command) in it. */
function inEnvironment(run) {
  const dark = opts.scheme === "dark";
  const scheme = [
    `export E2E_SCHEME=${opts.scheme}`,
    // GTK's (WebKit's prefers-color-scheme follows it), and GNOME's own.
    ...(dark ? ["export GTK_THEME=Adwaita:dark"] : []),
  ];
  // NVIDIA's libraries chosen over Mesa's; with dri, Mesa's are the only ones there.
  const gpu =
    opts.gpu === "nvidia"
      ? [
          "export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/10_nvidia.json GBM_BACKEND=nvidia-drm __GLX_VENDOR_LIBRARY_NAME=nvidia",
        ]
      : [];
  const runtime = 'export XDG_RUNTIME_DIR=$(mktemp -d) && chmod 700 "$XDG_RUNTIME_DIR"';
  const quoted = run.replaceAll("'", "'\\''");
  if (opts.env === "weston")
    return [
      ...scheme,
      ...gpu,
      runtime,
      "weston --backend=headless --renderer=gl --width=1400 --height=900 --socket=wl-e2e --debug > /out/weston.log 2>&1 &",
      "sleep 3",
      "unset DISPLAY; export WAYLAND_DISPLAY=wl-e2e GDK_BACKEND=wayland E2E_DISPLAY=weston",
      `dbus-run-session -- sh -c '${quoted}'`,
    ].join("\n");
  if (opts.env === "gnome")
    return [
      ...scheme,
      ...gpu,
      runtime,
      // Its own settings (a keyfile: no dconf service needed) and extensions.
      "export GSETTINGS_BACKEND=keyfile XDG_CONFIG_HOME=/tmp/gnome-config",
      "mkdir -p $XDG_DATA_HOME/gnome-shell/extensions && cp -r /repo/tests/e2e/gnome-shell/vodscribe-tests@vodscribe $XDG_DATA_HOME/gnome-shell/extensions/",
      "gsettings set org.gnome.shell disable-user-extensions false",
      `gsettings set org.gnome.shell enabled-extensions "['vodscribe-tests@vodscribe']"`,
      "gsettings set org.gnome.desktop.screensaver lock-enabled false",
      "gsettings set org.gnome.desktop.session idle-delay 0",
      ...(dark ? ["gsettings set org.gnome.desktop.interface color-scheme prefer-dark"] : []),
      // A system bus of its own (the Shell asks for one; no logind on it: it copes).
      "dbus-daemon --session --nofork --address=unix:path=$XDG_RUNTIME_DIR/system_bus > /dev/null 2>&1 &",
      "export DBUS_SYSTEM_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/system_bus",
      "unset DISPLAY",
      `dbus-run-session -- sh -c 'gnome-shell --headless --wayland --no-x11 --virtual-monitor 1400x900 --wayland-display wl-e2e > /out/shell.log 2>&1 & sleep 8; export WAYLAND_DISPLAY=wl-e2e GDK_BACKEND=wayland E2E_DISPLAY=gnome; ${quoted}'`,
    ].join("\n");
  return [
    ...scheme,
    ...gpu,
    `dbus-run-session -- xvfb-run -a -s "-screen 0 1400x900x24" sh -c 'openbox & sleep 1; ${quoted}'`,
  ].join("\n");
}

const summary = [];
for (const name of names) {
  const distro = DISTROS[name];
  if (!distro) throw new Error(`no distro ${name} (there are: ${Object.keys(DISTROS).join(", ")})`);
  const pkg = pkgs[distro.kind];
  if (!pkg) throw new Error(`no .${distro.kind} in ${packages}`);
  const out = path.join(here, "out", "distros", cell(name));
  const data = path.join(work, `${cell(name)}-data`, "app.vodscribe");
  rmSync(out, { recursive: true, force: true });
  rmSync(path.dirname(data), { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  mkdirSync(data, { recursive: true });
  for (const kept of ["bin", "models"]) {
    const from = process.env.E2E_SEED && path.join(process.env.E2E_SEED, kept);
    if (from && existsSync(from)) cpSync(from, path.join(data, kept), { recursive: true });
  }
  const quote = (a) => `'${a.replaceAll("'", "'\\''")}'`;
  const args = rest.map(quote).join(" ");
  const script = [
    "set -e",
    `EXTRA="${[...ENV_PACKAGES[opts.env][distro.pm], ...(opts.gpu ? GPU_PACKAGES[opts.gpu][distro.pm] : [])].join(" ")}"`,
    distro.install,
    "export XDG_DATA_HOME=/data E2E_RESET_DATA=1 VODSCRIBE_NO_UPDATE_CHECK=1 E2E_OUT=/out TAURI_DRIVER=/opt/tools/tauri-driver",
    // Passed on: scenarios that need the network, screenshots kept.
    ...["E2E_NETWORK", "E2E_SHOTS", "E2E_SITE"]
      .filter((k) => process.env[k])
      .map((k) => `export ${k}=${quote(process.env[k])}`),
    `echo "installed: $(. /etc/os-release; echo $PRETTY_NAME)"`,
    // An app that can't load its libraries never starts: say which, rather than time out each scenario.
    `missing=$(ldd /usr/bin/vodscribe /usr/bin/vodscribe-worker | grep "not found" || true); if [ -n "$missing" ]; then echo "missing libraries: $missing" | tee /out/run.log; exit 3; fi`,
    "cd /repo",
    `(${inEnvironment(`exec /opt/node/bin/node tests/e2e/run.mjs /usr/bin/vodscribe ${args}`)}) > /out/run.log 2>&1`,
  ].join("\n");
  console.log(`== ${cell(name)} (${distro.image}, .${distro.kind})`);
  const run = spawnSync(
    podman[0],
    [
      ...podman.slice(1),
      "run",
      "--rm",
      ...(distro.podman ?? []),
      ...(opts.gpu ? GPU_DEVICES[opts.gpu] : []),
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
  const skipped = results.filter((r) => r.ok && r.skipped);
  const line =
    results.length === 0
      ? `no results (exit ${run.status}): see ${path.join(out, "run.log")}`
      : `${results.length - failed.length - skipped.length} of ${results.length} passed` +
        (skipped.length ? `; skipped: ${skipped.map((r) => r.name).join(", ")}` : "") +
        (failed.length ? `; failed: ${failed.map((r) => r.name).join(", ")}` : "");
  console.log(`   ${line}`);
  summary.push(`${cell(name)}: ${line}`);
  rmSync(path.dirname(data), { recursive: true, force: true });
}
console.log(`\n${summary.join("\n")}`);
