import type { AppDefinition } from "rendrix/types";

type CounterState = { count: number };

// Runs in QuickJS inside the native Rendrix process, not in a browser.
const app: AppDefinition<CounterState> = {
  state: { count: 0 },
  actions: {
    increment(api) {
      api.set("count", api.get("count") + 1);
    },
    decrement(api) {
      api.set("count", api.get("count") - 1);
    },
    reset(api) {
      api.set("count", 0);
    }
  }
};

export default app;
