export const SITE = "https://vodscribe.com";
export const PRIVACY = `${SITE}/privacy`;
/** Every version's changes; `releaseNotes` links to one. */
export const RELEASES = `${SITE}/releases`;
/** Version `version`'s entry on the releases page ("0.2.1", or the demo's "0.2.1_demo"). */
export const releaseNotes = (version: string) => `${RELEASES}#${version.split(/[_+-]/)[0]}`;
/** Credits and licenses; the installers also include THIRD-PARTY-NOTICES.txt. */
export const LICENSES = `${SITE}/licenses`;
export const AUTHOR = "Airodonack";
/** Where "Made by" links: donations that cover hosting and code signing. */
export const SUPPORT = "https://ko-fi.com/airodonack";
/** Bug reports and ideas; a public tracker, no account needed to read it. */
export const ISSUES = "https://github.com/Airodonack/VODscribe/issues";
