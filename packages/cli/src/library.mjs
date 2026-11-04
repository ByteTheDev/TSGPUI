// Built-in components. Apps use them with <use component="Sidebar">...</use>. An app's own
// <template data-component="..."> with the same name replaces the library entry. The library is
// JavaScript rather than a file read at run time, so it ships inside the bundled CLI.
export const COMPONENT_LIBRARY_HTML = `<div>
  <template data-component="Sidebar"><section class="ts-sidebar" role="navigation"><slot></slot></section></template>
  <template data-component="Toolbar"><header class="ts-toolbar" role="toolbar"><div class="ts-toolbar-start"><slot name="start"></slot></div><div class="ts-toolbar-title"><slot>{{props.title}}</slot></div><div class="ts-toolbar-end"><slot name="end"></slot></div></header></template>
  <template data-component="SegmentedControl"><div class="ts-segmented" role="group"><slot></slot></div></template>
  <template data-component="SearchField"><input type="search" class="ts-search" placeholder="Search"></template>
  <template data-component="ListRow"><button class="ts-list-row"><span class="ts-list-title">{{props.title}}</span><span class="ts-list-detail"><slot></slot></span></button></template>
  <template data-component="Table"><section class="ts-table" role="table"><slot></slot></section></template>
  <template data-component="TableHeader"><header class="ts-table-header" role="row"><slot></slot></header></template>
  <template data-component="TableRow"><div class="ts-table-row" role="row"><slot></slot></div></template>
  <template data-component="TableCell"><div class="ts-table-cell" role="cell"><slot></slot></div></template>
  <template data-component="TableHeaderCell"><div class="ts-table-cell ts-table-header-cell" role="columnheader"><slot></slot></div></template>
  <template data-component="CodeEditor"><textarea class="ts-code-editor"></textarea></template>
  <template data-component="EmptyState"><section class="ts-empty-state"><h3 class="ts-empty-title">{{props.title}}</h3><div class="ts-empty-body"><slot></slot></div></section></template>
  <template data-component="Sheet"><div class="ts-sheet" role="dialog"><h3 class="ts-sheet-title">{{props.title}}</h3><div class="ts-sheet-body"><slot></slot></div><div class="ts-sheet-footer"><slot name="footer"></slot></div></div></template>
</div>`;

// Plain class selectors only, so the library never restyles an element that the app did not ask for.
// Colors come from custom properties, which an app's :root overrides.
export const COMPONENT_LIBRARY_CSS = `
:root {
  --ts-accent: #0a84ff;
  --ts-text: #1d1d1f;
  --ts-secondary: #6e6e73;
  --ts-surface: #f5f5f7;
  --ts-raised: #ffffff;
  --ts-control: #e8e8ed;
  --ts-control-pressed: #d1d1d6;
}
.ts-sidebar { width: 240px; flex-shrink: 0; padding: 12px 8px; gap: 2px; background: var(--ts-surface); }
.ts-toolbar { flex-direction: row; align-items: center; gap: 8px; height: 44px; padding: 0 12px; background: var(--ts-raised); }
.ts-toolbar-start, .ts-toolbar-end { flex-direction: row; align-items: center; gap: 6px; }
.ts-toolbar-title { flex-grow: 1; text-align: center; font-size: 13px; font-weight: 600; color: var(--ts-text); }
.ts-segmented { flex-direction: row; gap: 2px; padding: 2px; border-radius: 8px; background: var(--ts-control); }
.ts-segmented > button { height: 26px; padding: 0 12px; border-radius: 6px; font-size: 13px; color: var(--ts-text); background: transparent; align-items: center; justify-content: center; }
.ts-segmented > button:hover { background: var(--ts-control-pressed); }
.ts-segmented > button:active { background: #c7c7cc; }
.ts-segmented > button.ts-selected { background: var(--ts-raised); box-shadow: 0px 1px 2px 0px #00000026; }
.ts-search { width: 220px; height: 28px; padding: 0 10px; border-radius: 8px; background: var(--ts-control); color: var(--ts-text); font-size: 13px; }
.ts-list-row { flex-direction: row; align-items: center; justify-content: space-between; gap: 8px; height: 36px; padding: 0 10px; border-radius: 8px; background: transparent; color: var(--ts-text); font-size: 13px; }
.ts-list-row:hover { background: var(--ts-control); }
.ts-list-row:active { background: var(--ts-control-pressed); }
.ts-list-row.ts-selected { background: var(--ts-accent); color: #ffffff; }
.ts-list-title { flex-grow: 1; }
.ts-list-detail { color: var(--ts-secondary); font-size: 12px; }
.ts-table { width: 100%; gap: 1px; overflow: hidden; border-radius: 8px; background: var(--ts-control); }
.ts-table-header, .ts-table-row { flex-direction: row; align-items: center; gap: 1px; background: var(--ts-raised); }
.ts-table-header { background: var(--ts-surface); }
.ts-table-cell { flex-grow: 1; min-width: 0; padding: 9px 12px; font-size: 13px; color: var(--ts-text); }
.ts-table-header-cell { font-size: 12px; font-weight: 600; color: var(--ts-secondary); }
.ts-code-editor { width: 100%; min-height: 220px; padding: 12px; border-radius: 8px; background: #17191e; color: #e6e8ee; font-family: monospace; font-size: 13px; }
.ts-empty-state { align-items: center; justify-content: center; gap: 8px; padding: 28px; text-align: center; }
.ts-empty-title { font-size: 16px; font-weight: 600; color: var(--ts-text); }
.ts-empty-body { font-size: 13px; color: var(--ts-secondary); }
.ts-sheet { width: 360px; padding: 20px; gap: 14px; border-radius: 14px; background: var(--ts-raised); box-shadow: 0px 12px 32px 0px #00000040; }
.ts-sheet-title { font-size: 17px; font-weight: 600; color: var(--ts-text); }
.ts-sheet-body { gap: 8px; font-size: 13px; color: var(--ts-secondary); }
.ts-sheet-footer { flex-direction: row; justify-content: flex-end; gap: 8px; }
`;
