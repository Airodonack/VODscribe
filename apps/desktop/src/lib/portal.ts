// Moves an element to the end of <body>: for a panel that must show above
// everything, wherever it's declared. Inside a sticky header in a scrolling
// area, even a fixed-position panel can be clipped by that area, or drawn under
// a playing video (WebKit), whatever its z-index.
export function portal(node: HTMLElement) {
  document.body.appendChild(node);
  return { destroy: () => node.remove() };
}
