import type { AppDefinition } from "tsgpu/types";

type Account = { name: string; email: string; plan: string };

type FormState = {
  name: string;
  email: string;
  age: string;
  plan: string;
  newsletter: boolean;
  status: string;
  accounts: Account[];
  count: number;
};

const EMPTY_FORM = { name: "", email: "", age: "", plan: "free", newsletter: false };

// Returns a message for the first invalid field, or null when the form is fine.
function validate(form: Pick<FormState, "name" | "email" | "age">): string | null {
  if (form.name.trim().length < 2) return "Name needs at least 2 characters.";
  if (!/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(form.email.trim())) return "That email address does not look right.";
  if (form.age !== "") {
    const age = Number(form.age);
    if (!Number.isInteger(age) || age < 13 || age > 120) return "Age must be a whole number from 13 to 120.";
  }
  return null;
}

function clearForm(api: { set: (key: keyof FormState, value: never) => void }): void {
  for (const [key, value] of Object.entries(EMPTY_FORM)) api.set(key as keyof FormState, value as never);
}

const app: AppDefinition<FormState, "submit" | "reset" | "clearStatus"> = {
  state: { ...EMPTY_FORM, status: "Fill in the form and press Create.", accounts: [], count: 0 },
  actions: {
    submit(api) {
      const problem = validate({ name: api.get("name"), email: api.get("email"), age: api.get("age") });
      if (problem) {
        api.set("status", problem);
        return;
      }
      const account = { name: api.get("name").trim(), email: api.get("email").trim(), plan: api.get("plan") };
      api.update("accounts", (accounts) => [...accounts, account]);
      api.set("count", api.get("accounts").length);
      clearForm(api);
      api.set("status", `Created an account for ${account.name}.`);
    },
    reset(api) {
      clearForm(api);
      api.set("status", "Cleared.");
    },
    clearStatus(api) {
      api.set("status", "");
    }
  }
};

export default app;
