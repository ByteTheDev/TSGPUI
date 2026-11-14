//! The user-agent stylesheet: default presentation for HTML elements.

pub const USER_AGENT_CSS: &str = r#"
html, body, div, section, article, aside, header, footer, main, nav, p, h1, h2, h3, h4, h5, h6,
ul, ol, dl, dt, dd, li, form, fieldset, legend, figure, figcaption, blockquote, pre, hr, address,
details, summary, menu, dialog, table, caption { display: block; }
head, script, style, title, meta, link, template, noscript, datalist, base, option, optgroup { display: none; }
html { color: #000; font-size: 16px; }
body { margin: 8px; }
h1 { font-size: 2em; margin: 0.67em 0; font-weight: 700; }
h2 { font-size: 1.5em; margin: 0.83em 0; font-weight: 700; }
h3 { font-size: 1.17em; margin: 1em 0; font-weight: 700; }
h4 { margin: 1.33em 0; font-weight: 700; }
h5 { font-size: 0.83em; margin: 1.67em 0; font-weight: 700; }
h6 { font-size: 0.67em; margin: 2.33em 0; font-weight: 700; }
p, blockquote, figure, dl { margin: 1em 0; }
blockquote { margin: 1em 40px; }
ul, ol, menu { margin: 1em 0; padding-left: 40px; }
dd { margin-left: 40px; }
fieldset { margin: 0 2px; padding: 0.35em 0.75em 0.625em; border: 2px groove #c0c0c0; }
b, strong { font-weight: 700; }
i, em, cite, var, dfn { font-style: italic; }
u, ins { text-decoration: underline; }
s, strike, del { text-decoration: line-through; }
small { font-size: smaller; }
a { color: #0000ee; text-decoration: underline; cursor: pointer; }
code, kbd, samp, pre, tt { font-family: monospace; }
pre { white-space: pre; margin: 1em 0; }
hr { height: 0; margin: 0.5em 0; border-top: 1px solid #9a9a9a; }
img, canvas, svg, video { display: inline-block; }
button { display: inline-block; padding: 1px 6px; font-family: system-ui, sans-serif; font-size: 13.3333px; color: #000;
         background-color: #efefef; border: 1px solid #767676; border-radius: 3px; text-align: center; cursor: default; }
button:hover { background-color: #e5e5e5; border-color: #4f4f4f; }
button:active { background-color: #d0d0d0; border-color: #767676; }
button:disabled { color: #888; background-color: #f5f5f5; border-color: #c4c4c4; }
input, textarea, select { display: inline-block; width: 12.5em; min-height: 1.5em; padding: 1px 2px; font-family: system-ui, sans-serif; font-size: 13.3333px;
         color: #000; background-color: #fff; border: 1px solid #767676; border-radius: 2px; cursor: text; }
input:focus, textarea:focus, select:focus { outline: 2px solid #1a73e8; }
input:disabled, textarea:disabled, select:disabled { color: #888; background-color: #f0f0f0; border-color: #c4c4c4; }
input[type="checkbox"], input[type="radio"] { width: 13px; height: 13px; padding: 0; margin: 3px 3px 3px 4px; }
input[type="checkbox"]:checked, input[type="radio"]:checked { background-color: #1a73e8; border-color: #1a73e8; }
input[type="button"], input[type="submit"] { text-align: center; cursor: default; background-color: #efefef; }
textarea { white-space: pre-wrap; font-family: monospace; height: 4.5em; }
select { width: auto; min-width: 6em; padding-right: 1.4em; }
input[type="button"], input[type="submit"], input[type="reset"] { width: auto; }
input[type="checkbox"], input[type="radio"] { min-height: 0; }
input[type="radio"] { border-radius: 50%; }
ul, ol, menu { list-style: disc; }
label { cursor: default; }
[hidden] { display: none; }
"#;
