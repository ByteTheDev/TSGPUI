(function () {
  "use strict";

  var root = document.documentElement;
  root.classList.add("js");

  var reduceMotion = false;
  try {
    reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch (error) {
    reduceMotion = false;
  }

  // ---------------------------------------------------------------------------
  // Scroll reveals. Content is visible unless this script has added .js,
  // so a failure here never leaves sections hidden.
  // ---------------------------------------------------------------------------
  var reveals = document.querySelectorAll(".reveal");

  if (reduceMotion || !("IntersectionObserver" in window)) {
    reveals.forEach(function (el) {
      el.classList.add("is-in");
    });
  } else {
    var revealObserver = new IntersectionObserver(
      function (entries, observer) {
        entries.forEach(function (entry) {
          if (entry.isIntersecting) {
            entry.target.classList.add("is-in");
            observer.unobserve(entry.target);
          }
        });
      },
      { threshold: 0.12, rootMargin: "0px 0px -6% 0px" }
    );
    reveals.forEach(function (el) {
      revealObserver.observe(el);
    });
  }

  // ---------------------------------------------------------------------------
  // Syntax highlighting for the demo editor.
  // Rules use only non-capturing groups, so match indexes map to rule order.
  // ---------------------------------------------------------------------------
  var RULES = {
    html: [
      ["tk-com", /<!--[\s\S]*?-->/],
      ["tk-meta", /<!doctype[^>]*>/i],
      ["tk-str", /"[^"]*"/],
      ["tk-tag", /<\/?[a-zA-Z][\w-]*|\/?>/],
      ["tk-attr", /[\w-]+(?==)/]
    ],
    css: [
      ["tk-com", /\/\*[\s\S]*?\*\//],
      ["tk-str", /"[^"]*"/],
      ["tk-num", /#[0-9a-fA-F]{3,6}\b|\b\d+(?:\.\d+)?(?:px|em|rem|%|vh|vw)?\b/],
      ["tk-prop", /[a-z-]+(?=\s*:)/],
      ["tk-sel", /[.#][\w-]+|\b(?:html|body|h1|h2|h3|p|span|div|button|main|section|h1)\b/]
    ],
    ts: [
      ["tk-com", /\/\/[^\n]*/],
      ["tk-str", /"[^"\n]*"|'[^'\n]*'|`[^`]*`/],
      ["tk-kw", /\b(?:import|export|default|type|const|let|return|from)\b/],
      ["tk-type", /\b(?:AppDefinition|CounterState|number|string|boolean)\b/],
      ["tk-fn", /\b[A-Za-z_]\w*(?=\s*\()/],
      ["tk-num", /\b\d+\b/]
    ]
  };

  function escapeHtml(text) {
    return text
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
  }

  function highlight(code, lang) {
    var rules = RULES[lang];
    if (!rules) {
      return escapeHtml(code);
    }

    var pattern = new RegExp(
      rules
        .map(function (rule) {
          return "(" + rule[1].source + ")";
        })
        .join("|"),
      "g"
    );

    var out = "";
    var last = 0;
    var match;

    while ((match = pattern.exec(code)) !== null) {
      if (match[0].length === 0) {
        pattern.lastIndex += 1;
        continue;
      }

      out += escapeHtml(code.slice(last, match.index));

      var ruleIndex = 0;
      for (var i = 0; i < rules.length; i += 1) {
        if (match[i + 1] !== undefined) {
          ruleIndex = i;
          break;
        }
      }

      out +=
        '<span class="' + rules[ruleIndex][0] + '">' + escapeHtml(match[0]) + "</span>";
      last = match.index + match[0].length;
    }

    return out + escapeHtml(code.slice(last));
  }

  document.querySelectorAll("code[data-lang]").forEach(function (block) {
    block.innerHTML = highlight(block.textContent, block.getAttribute("data-lang"));
  });

  // ---------------------------------------------------------------------------
  // Code tabs with roving focus and arrow-key support.
  // ---------------------------------------------------------------------------
  var tablist = document.querySelector('[role="tablist"]');

  if (tablist) {
    var tabs = Array.prototype.slice.call(tablist.querySelectorAll('[role="tab"]'));

    function selectTab(tab, moveFocus) {
      tabs.forEach(function (other) {
        var selected = other === tab;
        other.setAttribute("aria-selected", selected ? "true" : "false");
        other.setAttribute("tabindex", selected ? "0" : "-1");

        var panel = document.getElementById(other.getAttribute("aria-controls"));
        if (panel) {
          panel.hidden = !selected;
        }
      });

      if (moveFocus) {
        tab.focus();
      }
    }

    tabs.forEach(function (tab, index) {
      tab.addEventListener("click", function () {
        selectTab(tab, false);
      });

      tab.addEventListener("keydown", function (event) {
        var next = null;

        if (event.key === "ArrowRight") {
          next = tabs[(index + 1) % tabs.length];
        } else if (event.key === "ArrowLeft") {
          next = tabs[(index - 1 + tabs.length) % tabs.length];
        } else if (event.key === "Home") {
          next = tabs[0];
        } else if (event.key === "End") {
          next = tabs[tabs.length - 1];
        }

        if (next) {
          event.preventDefault();
          selectTab(next, true);
        }
      });
    });
  }

  // ---------------------------------------------------------------------------
  // Copy buttons. Either copy a code element's text or a data-copy-text value.
  // ---------------------------------------------------------------------------
  document.querySelectorAll(".copy").forEach(function (button) {
    button.addEventListener("click", function () {
      var text = "";
      var fromId = button.getAttribute("data-copy-from");
      var literal = button.getAttribute("data-copy-text");

      if (button.hasAttribute("data-copy-active")) {
        var visible = button.closest(".editor").querySelector(".panel:not([hidden]) code");
        text = visible ? visible.textContent : "";
      } else if (fromId) {
        var source = document.getElementById(fromId);
        text = source ? source.textContent : "";
      } else if (literal !== null) {
        text = literal;
      } else {
        var block = button.parentElement.querySelector("code");
        text = block ? block.textContent : "";
      }

      var original = button.textContent;

      function done(label) {
        button.textContent = label;
        button.classList.toggle("is-done", label === "Copied");
        window.setTimeout(function () {
          button.textContent = original;
          button.classList.remove("is-done");
        }, 1600);
      }

      try {
        navigator.clipboard.writeText(text).then(
          function () {
            done("Copied");
          },
          function () {
            done("Copy failed");
          }
        );
      } catch (error) {
        done("Copy failed");
      }
    });
  });

  // ---------------------------------------------------------------------------
  // Simulated counter. This mirrors the UI of examples/counter. It runs in
  // the page and does not use the TSGPUI runtime; the page says so.
  // ---------------------------------------------------------------------------
  document.querySelectorAll("[data-counter]").forEach(function (app) {
    var countEl = app.querySelector("[data-count]");
    var value = 0;

    function render() {
      countEl.textContent = String(value);
      countEl.classList.add("bump");
      window.setTimeout(function () {
        countEl.classList.remove("bump");
      }, 180);
    }

    app.querySelectorAll("[data-action]").forEach(function (button) {
      button.addEventListener("click", function () {
        var action = button.getAttribute("data-action");

        if (action === "increment") {
          value += 1;
        } else if (action === "decrement") {
          value -= 1;
        } else if (action === "reset") {
          value = 0;
        }

        render();
      });
    });
  });

  // ---------------------------------------------------------------------------
  // Pipeline: run the sequenced highlight once when it scrolls into view.
  // ---------------------------------------------------------------------------
  var pipeline = document.querySelector("[data-pipeline]");

  if (pipeline && !reduceMotion && "IntersectionObserver" in window) {
    var pipelineObserver = new IntersectionObserver(
      function (entries, observer) {
        entries.forEach(function (entry) {
          if (entry.isIntersecting) {
            pipeline.classList.add("is-running");
            observer.disconnect();
          }
        });
      },
      { threshold: 0.35 }
    );
    pipelineObserver.observe(pipeline);
  }

  // ---------------------------------------------------------------------------
  // Cursor-following light on [data-spot] surfaces.
  // ---------------------------------------------------------------------------
  if (!reduceMotion && window.matchMedia("(hover: hover)").matches) {
    document.querySelectorAll("[data-spot]").forEach(function (el) {
      el.addEventListener("pointermove", function (event) {
        var rect = el.getBoundingClientRect();
        el.style.setProperty("--mx", event.clientX - rect.left + "px");
        el.style.setProperty("--my", event.clientY - rect.top + "px");
      });
    });
  }

  // ---------------------------------------------------------------------------
  // Mark the navigation link for the section currently in view.
  // ---------------------------------------------------------------------------
  var navLinks = document.querySelectorAll(".nav-links a[href^='#']");
  var sectionIds = Array.prototype.map.call(navLinks, function (link) {
    return link.getAttribute("href").slice(1);
  });

  if ("IntersectionObserver" in window) {
    var activeObserver = new IntersectionObserver(
      function (entries) {
        entries.forEach(function (entry) {
          if (!entry.isIntersecting) {
            return;
          }
          navLinks.forEach(function (link) {
            var current = link.getAttribute("href") === "#" + entry.target.id;
            if (current) {
              link.setAttribute("aria-current", "true");
            } else {
              link.removeAttribute("aria-current");
            }
          });
        });
      },
      { rootMargin: "-45% 0px -50% 0px" }
    );

    sectionIds.forEach(function (id) {
      var section = document.getElementById(id);
      if (section) {
        activeObserver.observe(section);
      }
    });
  }
})();
