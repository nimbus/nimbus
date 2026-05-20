// Auth-surface copy-to-clipboard binding. Mirrors the React CopyChip
// component (packages/nimbus-ui/src/components/copy-chip.tsx) so every
// machine-readable value on the sign-in page honours the DESIGN.md rule
// that commands and identifiers are one click from the user's clipboard.
//
// Selects by `[data-copy]` rather than `.copyable` so the same behaviour
// powers full chip variants (`.copyable`) and ghost variants
// (`.brand-version`, `.local-host`) without each one having to re-bind.
//
// Served as a same-origin asset at `/ui/auth.js` so it satisfies the
// `script-src 'self'` CSP without needing a hash pin or `'unsafe-inline'`.
(function () {
  var COPIED_MS = 1200;

  function fallbackCopy(value, done) {
    var ta = document.createElement("textarea");
    ta.value = value;
    ta.setAttribute("readonly", "");
    ta.style.position = "absolute";
    ta.style.left = "-9999px";
    document.body.appendChild(ta);
    ta.select();
    try {
      document.execCommand("copy");
      done();
    } catch (_) {}
    document.body.removeChild(ta);
  }

  function bind() {
    document.querySelectorAll("button[data-copy]").forEach(function (btn) {
      var value = btn.dataset.copy || btn.textContent.trim();
      if (!btn.hasAttribute("aria-label")) {
        btn.setAttribute("aria-label", "Copy: " + value);
      }
      btn.addEventListener("click", function () {
        var done = function () {
          btn.setAttribute("data-copied", "true");
          setTimeout(function () {
            btn.removeAttribute("data-copied");
          }, COPIED_MS);
        };
        if (navigator.clipboard && navigator.clipboard.writeText) {
          navigator.clipboard.writeText(value).then(done).catch(function () {
            fallbackCopy(value, done);
          });
        } else {
          fallbackCopy(value, done);
        }
      });
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", bind);
  } else {
    bind();
  }
})();
