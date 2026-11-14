# Notes example

A multi-file, typed backend that saves text files through the native bridge. The entry point is `src/backend.ts`; `src/notes.ts` holds reusable validation and error helpers.

Create a `notes` folder beside the packaged executable, or beside the `.rdx` bundle when running with `--bundle` (including `dev`), before using the app. The configuration grants reads and writes only inside that folder. Save or open a note by entering its name; Refresh lists saved files. Failures, including missing folders and denied access, appear in the status line. Native bridge operations return promises, so actions await them inside `try`/`catch`.

From the repository root, run `node packages/cli/bin/rendrix.mjs compile --project examples/notes` to compile, or use `build`/`dev` with a compatible native host. Run `tsc --noEmit -p examples/notes/tsconfig.json` separately to type-check the backend.
