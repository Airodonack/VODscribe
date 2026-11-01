export { AUTHOR, ISSUES, SOURCE } from "desktop/lib/links.ts";

/** Why the installers warn, said on the page and in the download note. */
export const UNSIGNED = "isn't code-signed yet: signing needs a registered company, and I'm setting one up now.";

/** The VODscriptions page: the site's front for streamers. Its header and the
 *  main site's nav read this, so moving the page (to a subdomain, say) is
 *  this constant and a deploy. */
export const PREMIUM = "/vodscriptions";

/** Where "Send me an email if you're interested" writes to, until sign-up opens. */
export const CONTACT_EMAIL = "hello@vodscribe.com";
