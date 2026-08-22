// Ambient JSX namespace paired with the classic (`jsxFactory: "h"`) JSX
// transform configured in tsconfig.json and runtime/jsx.tsx's `h`/
// `Fragment`. There's no DOM/React here, so this doesn't attempt to type
// individual intrinsic elements (`<div>`, `<span>`, ...) against a known
// element catalog — any tag name is accepted, with any props.

declare namespace JSX {
    /** What `h(...)`/JSX expressions evaluate to. */
    type Element = import("../runtime/jsx").VNode;

    interface IntrinsicElements {
        [tagName: string]: Record<string, unknown>;
    }
}

// src/runtime.rs bootstraps runtime/jsx.tsx's `h`/`Fragment` exports onto
// every program's global scope, so the classic JSX transform above can find
// them without a program ever writing `import { h, Fragment } from "jsx"`.
declare const h: (
    type: import("../runtime/jsx").VNodeType,
    props: import("../runtime/jsx").Props | null,
    ...children: unknown[]
) => JSX.Element;
declare const Fragment: (props: import("../runtime/jsx").Props) => JSX.Element;
