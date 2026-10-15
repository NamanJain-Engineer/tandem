// The docs' Next chunks, added once a frame has painted (D-1819,
// scripts/defer-docs.mjs). The list is this tag's data-chunks: one path per
// chunk, with "#id" when Next gave the tag an id.
(function () {
  var me = document.currentScript;
  var list = ((me && me.getAttribute("data-chunks")) || "").split(" ");
  function load() {
    for (var i = 0; i < list.length; i++) {
      if (!list[i]) continue;
      var part = list[i].split("#");
      var s = document.createElement("script");
      s.src = part[0];
      if (part[1]) s.id = part[1];
      s.async = true;
      document.body.appendChild(s);
    }
  }
  // The callback runs before the next paint; the timeout after it.
  requestAnimationFrame(function () {
    setTimeout(load, 0);
  });
})();
