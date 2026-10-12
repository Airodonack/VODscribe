// The website's live demo (apps/site) builds these same components with
// __DEMO__ defined as true. In the app it's undefined, and everything here is a no-op.

declare const __DEMO__: boolean | undefined;

/** Whether this is the website's demo rather than the app. */
export const DEMO = typeof __DEMO__ !== "undefined" && __DEMO__;

/**
 * Attributes for a control that only works in the app. The demo shows it
 * grayed out, with `message` on hover, and ignores clicks on it.
 */
export function demoOff(message: string): Record<string, string> {
  return DEMO ? { "data-demo": message, "aria-disabled": "true", "aria-description": message } : {};
}

/**
 * Whether a key press is meant for the app. In the demo, the page around it
 * keeps its keys (find, Space and arrows to scroll) until the visitor clicks
 * into the demo or focuses something in it; the site marks that with data-active.
 */
export function keysForApp(): boolean {
  if (!DEMO) return true;
  const root = document.querySelector("[data-demo-root]");
  return !!root && (root.hasAttribute("data-active") || root.contains(document.activeElement));
}
