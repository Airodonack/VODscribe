<script lang="ts">
  // Where a sign-in or sign-up will be: a button that can't be pressed yet,
  // and an email link for whoever wants to hear when it can (CONTACT_MAILTO).
  import { CONTACT_MAILTO } from "#lib/links.js";
</script>

<div class="soon">
  <button class="primary" disabled>Coming soon</button>
  <p class="small muted">
    <a href={CONTACT_MAILTO}>Send an email</a> to hear when it opens.
  </p>
</div>

<style>
  .soon {
    display: flex;
    flex-direction: column;
    align-items: inherit;
    gap: 8px;
  }

  button {
    padding: 11px 22px;
    border-radius: var(--radius);
    font-size: 16px;
    font-weight: 600;
  }

  button:disabled {
    cursor: default;
    opacity: 0.6;
  }

  p {
    margin: 0;
  }

  .small {
    font-size: 13px;
  }
</style>
