// The blog's one script (F-WB-06, D-1860): a Copy button on each code block.
// The buttons are in the page already, hidden; without this script, or
// without clipboard access, they stay hidden and the code is still there to
// select. First-party, no requests, nothing stored.
(function () {
  if (!navigator.clipboard || !window.isSecureContext) return;
  var blocks = document.querySelectorAll("[data-code]");
  for (var i = 0; i < blocks.length; i++) setUp(blocks[i]);

  function setUp(block) {
    var button = block.querySelector("button[data-copy]");
    var pre = block.querySelector("pre");
    var status = block.querySelector("[data-copy-status]");
    if (!button || !pre) return;
    var label = button.querySelector("[data-copy-label]") || button;
    var timer = 0;
    button.hidden = false;
    button.addEventListener("click", function () {
      navigator.clipboard.writeText(pre.innerText.replace(/\n$/, "")).then(
        function () {
          show("Copied", "Code copied to the clipboard");
        },
        function () {
          show("Copy failed", "Couldn't copy: select the code instead");
        },
      );
    });
    function show(text, said) {
      label.textContent = text;
      if (status) status.textContent = said;
      clearTimeout(timer);
      timer = setTimeout(function () {
        label.textContent = "Copy";
        if (status) status.textContent = "";
      }, 2000);
    }
  }
})();
