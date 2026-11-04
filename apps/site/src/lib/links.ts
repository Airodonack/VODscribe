import { SOURCE } from "desktop/lib/links.ts";

export { AUTHOR, ISSUES, SOURCE } from "desktop/lib/links.ts";

/** Why the installers warn, said on the page and in the download note. */
export const UNSIGNED = "isn't code-signed yet: signing needs a registered company, and I'm setting one up now.";

/** The VODscriptions page: the site's front for streamers. Its header and the
 *  main site's nav read this, so moving the page (to a subdomain, say) is
 *  this constant and a deploy. */
export const PREMIUM = "/vodscriptions";

/** The VODscriptions page's email link, until sign-up opens: one address and
 *  subject for every link on the page. */
export const CONTACT_MAILTO = "mailto:hello@vodscribe.com?subject=VODscriptions";

/** The one address for help: the site shows it, the app never does (spam), and
 *  links to the support page instead. */
export const SUPPORT_EMAIL = "support@vodscribe.com";

/** Where a VODscription's buyer reports a problem with one, from the
 *  VODscriptions page. */
export const SUPPORT_MAILTO = `mailto:${SUPPORT_EMAIL}?subject=VODscriptions`;

/** Help with the app, from the support, privacy and home pages. */
export const HELP_MAILTO = `mailto:${SUPPORT_EMAIL}?subject=VODscribe`;

/** A security problem, reported privately (as SECURITY.md says). */
export const SECURITY_REPORT = `${SOURCE}/security/advisories/new`;
