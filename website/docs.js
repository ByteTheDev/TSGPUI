(function () {
  var tabs = Array.prototype.slice.call(document.querySelectorAll("[data-doc]"));
  var body = document.getElementById("doc-content");
  if (!tabs.length || !body) return;

  var cache = {};

  function load(file) {
    if (!cache[file]) {
      cache[file] = fetch(file)
        .then(function (response) {
          if (!response.ok) throw new Error(response.status + " " + response.statusText);
          return response.text();
        })
        .catch(function (error) {
          // Opening index.html from disk blocks fetch(), so use the copy embedded by sync-docs.mjs.
          var embedded = window.RENDRIX_DOCS && window.RENDRIX_DOCS[file];
          if (embedded) return embedded;
          throw error;
        });
    }
    return cache[file];
  }

  function show(tab) {
    tabs.forEach(function (other) {
      var selected = other === tab;
      other.setAttribute("aria-selected", selected ? "true" : "false");
      other.setAttribute("tabindex", selected ? "0" : "-1");
    });

    var file = tab.getAttribute("data-doc");
    body.textContent = "Loading…";
    load(file)
      .then(function (markdown) {
        // Trusted repository content, so the rendered HTML is inserted directly.
        if (window.marked) {
          body.innerHTML = marked.parse(markdown);
        } else {
          // marked comes from a CDN; without it, show the source text rather than an empty tab.
          var source = document.createElement("pre");
          source.textContent = markdown;
          body.replaceChildren(source);
        }
        // The page has one h1; the document title is a section heading.
        body.querySelectorAll("h1").forEach(function (title) {
          var heading = document.createElement("h2");
          heading.innerHTML = title.innerHTML;
          title.replaceWith(heading);
        });
        body.scrollTop = 0;
      })
      .catch(function () {
        body.textContent =
          "Could not load " + file + ". Serve the website folder over HTTP to read the docs here.";
      });
  }

  tabs.forEach(function (tab) {
    tab.addEventListener("click", function () {
      show(tab);
    });
  });

  show(tabs[0]);
})();
