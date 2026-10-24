// The installers, on R2 under dl.vodscribe.com/releases/. Each release is
// uploaded to releases/<version>/ under versioned names and to releases/latest/
// under these stable names, so the buttons follow new releases without
// rebuilding the site (which is why the site doesn't show a version number).
// The latest/ copies are served with the versioned file name, so the saved
// file says which version it is.

/** The download host: the installers, and the VOD pages' annotations (/v/<platform>/<id>). */
export const DOWNLOAD_HOST = "https://dl.vodscribe.com";

const BASE = `${DOWNLOAD_HOST}/releases/latest`;

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
  { os: "linux", label: "Linux · Debian and Ubuntu", detail: "Package (.deb)", url: `${BASE}/VODscribe-linux-x64.deb` },
  {
    os: "linux",
    label: "Linux · Fedora and openSUSE",
    detail: "Package (.rpm)",
    url: `${BASE}/VODscribe-linux-x64.rpm`,
  },
];

/**
 * The installer to offer first for `os`: the .rpm where the browser names an
 * RPM distro (Fedora's and openSUSE's Firefox do), otherwise the OS's first.
 * For a Mac, only once `macChip` is known: an Apple Silicon app won't open on
 * an Intel Mac, and the other way around it runs slowly.
 */
export function primaryDownload(os: Os | null, macChip: MacChip | null = null): Download | undefined {
  if (os === "mac") return macChip ? DOWNLOADS.find((d) => d.url.includes(`macos-${macChip}`)) : undefined;
  const rpm = os === "linux" && /Fedora|SUSE|Red Hat/i.test(navigator.userAgent);
  return DOWNLOADS.find((d) => d.os === os && (!rpm || d.url.endsWith(".rpm")));
}

export type MacChip = "arm64" | "x64";

/**
 * Which chip this Mac has, where the browser says (Chrome and Edge do, when
 * asked); null otherwise. Every Mac browser's user agent says "Intel", so
 * that can't tell.
 */
export async function detectMacChip(): Promise<MacChip | null> {
  const data = (
    navigator as { userAgentData?: { getHighEntropyValues(h: string[]): Promise<{ architecture?: string }> } }
  ).userAgentData;
  try {
    const arch = (await data?.getHighEntropyValues(["architecture"]))?.architecture;
    return arch === "arm" ? "arm64" : arch === "x86" ? "x64" : null;
  } catch {
    return null;
  }
}

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
