// The installers, on R2 under dl.vodscribe.com/releases/. Each release is
// uploaded to releases/<version>/ and copied to releases/latest/ under these
// names, so the buttons follow new releases without rebuilding the site (which
// is why the site doesn't show a version number).

const BASE = "https://dl.vodscribe.com/releases/latest";

export type Os = "windows" | "mac" | "linux";

export interface Download {
  os: Os;
  label: string;
  detail: string;
  url: string;
}

export const DOWNLOADS: Download[] = [
  {
    os: "windows",
    label: "Windows",
    detail: "64-bit installer (.exe)",
    url: `${BASE}/VODscribe-windows-x64-setup.exe`,
  },
  {
    os: "windows",
    label: "Windows · MSI",
    detail: "For managed installs (.msi)",
    url: `${BASE}/VODscribe-windows-x64.msi`,
  },
  {
    os: "mac",
    label: "macOS · Apple Silicon",
    detail: "M1 and later (.dmg)",
    url: `${BASE}/VODscribe-macos-arm64.dmg`,
  },
  { os: "mac", label: "macOS · Intel", detail: "Older Macs (.dmg)", url: `${BASE}/VODscribe-macos-x64.dmg` },
  {
    os: "linux",
    label: "Linux",
    detail: "AppImage, runs on most distributions",
    url: `${BASE}/VODscribe-linux-x64.AppImage`,
  },
  { os: "linux", label: "Linux · Debian and Ubuntu", detail: "Package (.deb)", url: `${BASE}/VODscribe-linux-x64.deb` },
  {
    os: "linux",
    label: "Linux · Fedora and openSUSE",
    detail: "Package (.rpm)",
    url: `${BASE}/VODscribe-linux-x64.rpm`,
  },
];

/** This visitor's OS, or null for phones and anything else. */
export function detectOs(): Os | null {
  const ua = navigator.userAgent;
  if (/Android|iPhone|iPad|iPod/i.test(ua)) return null;
  // iPads say they're Macs, but have touch screens.
  if (/Macintosh/i.test(ua) && navigator.maxTouchPoints > 1) return null;
  if (/Windows/i.test(ua)) return "windows";
  if (/Mac OS X|Macintosh/i.test(ua)) return "mac";
  if (/Linux|X11/i.test(ua)) return "linux";
  return null;
}
