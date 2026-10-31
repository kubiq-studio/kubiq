// app.js — trivial demo tenant proving the bridge loop.
(function () {
  "use strict";

  var log = document.getElementById("log");
  var doc = document.getElementById("document");
  var send = document.getElementById("send");

  function append(frame) {
    log.textContent += JSON.stringify(frame) + "\n";
  }

  window.kubiq.on(function (frame) {
    append(frame);
    if (frame && frame.type === "open") {
      doc.value = frame.document || "";
    } else if (frame && frame.type === "reload") {
      doc.value = frame.document || "";
    }
  });

  send.addEventListener("click", function () {
    window.kubiq.post({ type: "changed", document: doc.value });
  });

  window.kubiq.post({ type: "ready" });
})();
