(function () {
  var tabs = Array.prototype.slice.call(document.querySelectorAll("[data-doc]"));
  var body = document.getElementById("doc-content");
  if (!tabs.length || !body) return;

  var cache = {};

  function load(file) {
    if (!cache[file]) {
      cache[file] = fetch(file).then(function (response) {
        if (!response.ok) throw new Error(response.status + " " + response.statusText);
        return response.text();
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
        body.innerHTML = window.marked ? marked.parse(markdown) : "";
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
