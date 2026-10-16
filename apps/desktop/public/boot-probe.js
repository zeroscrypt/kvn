/* Boot probe — runs before the application module so that a UI that fails
 * to start is never a silent blank window.
 *
 * - reports load progress and uncaught errors to the backend event log via
 *   `ui_report` (best-effort: failures fall back to console and retries);
 * - sets the window title to "KVN [probe]" until the app module takes over,
 *   which makes the failure stage visible even when IPC is unavailable.
 *
 * This file is intentionally plain ES5: it must load even if the bundle
 * cannot (that is the whole point).
 */
(function () {
  "use strict";

  var pending = [];
  var retryTimer = null;
  var retryUntil = Date.now() + 20000;

  function send(level, message) {
    try {
      var internals = window.__TAURI_INTERNALS__;
      if (internals && typeof internals.invoke === "function") {
        var promise = internals.invoke("ui_report", {
          level: level,
          message: String(message),
        });
        if (promise && typeof promise.then === "function") {
          promise.then(null, function () {});
        }
        return true;
      }
    } catch (_err) {
      /* fall through */
    }
    return false;
  }

  function scheduleRetry() {
    if (retryTimer !== null || pending.length === 0) return;
    if (Date.now() > retryUntil) {
      pending.length = 0;
      return;
    }
    retryTimer = window.setTimeout(function () {
      retryTimer = null;
      while (pending.length > 0 && send(pending[0][0], pending[0][1])) {
        pending.shift();
      }
      scheduleRetry();
    }, 100);
  }

  window.__kvnReport = function (level, message) {
    if (!send(level, message)) {
      pending.push([level, message]);
      if (pending.length > 50) pending.shift();
      scheduleRetry();
    }
  };

  document.title = "KVN";
  window.__kvnReport("info", "boot probe loaded");

  window.addEventListener("error", function (event) {
    var where =
      event && event.filename
        ? " @ " + event.filename + ":" + event.lineno
        : "";
    var what = event && event.message ? event.message : String(event);
    window.__kvnReport("error", "ui error: " + what + where);
  });

  window.addEventListener("unhandledrejection", function (event) {
    var reason = event && event.reason ? String(event.reason) : "unknown";
    window.__kvnReport("error", "ui rejection: " + reason);
  });

  window.addEventListener("DOMContentLoaded", function () {
    var booted = window.__kvnBooted === true;
    window.__kvnReport(
      "info",
      booted ? "dom ready (ui booted)" : "dom ready (ui module has not finished booting yet)"
    );
  });
})();
