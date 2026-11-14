/** The `rendrix` module: application, window, clipboard and (permission-gated) filesystem APIs. */
export const version: string;
export function quit(): void;
export const window: {
  setTitle(title: string): void;
  close(): void;
  resize(width: number, height: number): void;
};
export const clipboard: { readText(): Promise<string>; writeText(text: string): Promise<void> };
export const fs: {
  readTextFile(path: string): Promise<string>;
  writeTextFile(path: string, data: string): Promise<void>;
  exists(path: string): Promise<boolean>;
  readDir(path: string): Promise<string[]>;
};
declare const rendrix: { version: string; engine: "rendrix"; quit: typeof quit; window: typeof window; clipboard: typeof clipboard; fs: typeof fs };
export default rendrix;
