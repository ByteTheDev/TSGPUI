import { render } from "rendrix/jsx-runtime";

let clicks = 0;
const label = (<p>No clicks yet.</p>) as HTMLParagraphElement;

render(
  <main>
    <h1>Hello, Rendrix</h1>
    {label}
    <button
      onClick={() => {
        clicks += 1;
        label.textContent = `Clicked ${clicks} time${clicks === 1 ? "" : "s"}.`;
      }}
    >
      Click me
    </button>
  </main>,
  document.getElementById("root")!,
);
