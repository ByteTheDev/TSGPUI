/** JSX compiles to direct DOM construction: an element expression is a DOM `Node`. */
export namespace JSX {
  type Element = Node;
  interface ElementChildrenAttribute {
    children: {};
  }
  type Child = Node | string | number | boolean | null | undefined | Child[];
  interface IntrinsicAttributes {
    key?: string | number;
  }
  interface IntrinsicElements {
    [tag: string]: {
      children?: Child;
      class?: string | string[];
      className?: string;
      style?: string | Record<string, string | number>;
      ref?: ((el: any) => void) | { current: any };
      id?: string;
      [attr: string]: unknown;
    };
  }
}
export const Fragment: unique symbol;
export function jsx(type: string | ((props: any) => Node), props: Record<string, unknown> | null, key?: string): Node;
export function jsxs(type: string | ((props: any) => Node), props: Record<string, unknown> | null, key?: string): Node;
export function jsxDEV(type: string | ((props: any) => Node), props: Record<string, unknown> | null, key?: string): Node;
/** Replace a container's children with a node. */
export function render(node: Node | (() => Node), container: Element): Element;
