<script lang="ts">
  // Where a sign-in or sign-up will be: a button that can't be pressed yet,
  // and under it the waiting list's one action, an email (CONTACT_MAILTO)
  // set as a secondary button.
  import { CONTACT_MAILTO } from "#lib/links.js";
</script>

<div class="soon">
  <button class="primary" disabled>Coming soon</button>
  <a class="secondary" href={CONTACT_MAILTO}>Tell me when it opens</a>
</div>

<style>
  .soon {
    display: flex;
    flex-direction: column;
    align-items: inherit;
    gap: 10px;
  }

  button,
  .secondary {
    padding: 11px 22px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    font-size: 16px;
    font-weight: 600;
  }

  /* Solid rather than faded, so it still reads as the page's accent button
     while it can't be pressed: a tint of the accent with dark accent text
     (7.5:1), and its dark-theme twin (9.1:1). */
  button:disabled {
    /* app.css fades every disabled button; this one stays solid. */
    opacity: 1;
    cursor: not-allowed;
    background: #c7c5f7;
    border-color: #c7c5f7;
    color: #2a2585;
  }

  @media (prefers-color-scheme: dark) {
    button:disabled {
      background: #2f3270;
      border-color: #2f3270;
      color: #e0e2ff;
    }
  }

  /* Outlined: the accent on the card is over 6:1 in both themes. */
  .secondary {
    padding-block: 9px;
    border-color: var(--accent);
    color: var(--accent);
    font-size: 15px;
    text-decoration: none;
  }

  .secondary:hover {
    background: var(--accent-soft);
  }
</style>
