// Ambient JSX namespace paired with the classic (`jsxFactory: "h"`) JSX
// transform configured in tsconfig.json and jsx-runtime.ts's `h`/`Fragment`.
// There's no DOM/React here, so this doesn't attempt to type individual
// intrinsic elements (`<div>`, `<span>`, ...) against a known element
// catalog — any tag name is accepted, with any props.

declare namespace JSX {
    /** What `h(...)`/JSX expressions evaluate to. */
    type Element = import("./jsx-runtime").VNode;

    interface IntrinsicElements {
        [tagName: string]: Record<string, unknown>;
    }
}

// kernel/runtime.rs bootstraps jsx-runtime.ts's `h`/`Fragment` exports onto
// every program's global scope, so the classic JSX transform above can find
// them without a program ever writing `import { h, Fragment } from "jsx"`.
declare const h: (
    type: import("./jsx-runtime").VNodeType,
    props: import("./jsx-runtime").Props | null,
    ...children: unknown[]
) => JSX.Element;
declare const Fragment: (props: import("./jsx-runtime").Props) => JSX.Element;
