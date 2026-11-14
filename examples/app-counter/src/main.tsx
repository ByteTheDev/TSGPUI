// A complete Rendrix application: TypeScript -> ES modules -> Rendrix's own DOM, CSS, flexbox layout
// and GPU renderer. Nothing here runs in a browser.
import { render } from "rendrix/jsx-runtime";

interface State {
  count: number;
  log: { text: string; count: number }[];
}

const state: State = { count: 0, log: [] };

const countEl = (<span class="count" id="count">0</span>) as HTMLSpanElement;
const hintEl = (<span class="hint" id="hint">Click a button to update the interface.</span>) as HTMLSpanElement;
const panelEl = (<div class="counter" id="panel"></div>) as HTMLDivElement;
const logEl = (<ul class="log" id="log"></ul>) as HTMLUListElement;

function describe(count: number): string {
  if (count === 0) return "Click a button to update the interface.";
  return `${Math.abs(count)} ${count > 0 ? "above" : "below"} zero`;
}

/** Push state into the DOM; Rendrix restyles, relayouts and repaints on the next frame. */
function sync(action: string) {
  countEl.textContent = String(state.count);
  hintEl.textContent = describe(state.count);
  panelEl.classList.toggle("positive", state.count > 0);
  panelEl.classList.toggle("negative", state.count < 0);
  state.log.unshift({ text: action, count: state.count });
  state.log.length = Math.min(state.log.length, 3);
  logEl.replaceChildren(
    ...state.log.map((entry) => (
      <li>
        <span>{entry.text}</span>
        <span>{String(entry.count)}</span>
      </li>
    )),
  );
}

function change(delta: number) {
  state.count += delta;
  sync(delta > 0 ? "increase" : "decrease");
}

function reset() {
  state.count = 0;
  sync("reset");
}

panelEl.append(<span class="eyebrow">CLICK COUNTER</span>, countEl, hintEl);

render(
  <main class="shell">
    <section class="card">
      <div class="badge">RENDRIX · NATIVE RENDERING</div>
      <h1>Rendrix</h1>
      <p class="subtitle">TypeScript, DOM, CSS and flexbox rendered by Rendrix's own GPU pipeline. No browser engine.</p>
      {panelEl}
      <div class="buttons">
        <button id="decrement" class="secondary" onClick={() => change(-1)}>− Decrease</button>
        <button id="increment" class="primary" onClick={() => change(1)}>+ Increase</button>
        <button id="reset" class="ghost" onClick={reset}>Reset</button>
      </div>
      {logEl}
    </section>
  </main>,
  document.getElementById("root")!,
);

console.log("counter app mounted");
