// Make light mode explicit on <html> so sphinxcontrib-mermaid's theme
// detection doesn't fall through to prefers-color-scheme. sphinx-hextra
// only adds `html.dark`; without a matching `html.light` marker, a user
// who toggled to light on an OS set to dark would get mermaid in its
// "dark" theme on a light background (invisible text in dark-filled
// boxes).
(function () {
  "use strict";
  var root = document.documentElement;
  function sync() {
    root.classList.toggle("light", !root.classList.contains("dark"));
  }
  sync();
  new MutationObserver(sync).observe(root, {
    attributes: true,
    attributeFilter: ["class"],
  });
})();
