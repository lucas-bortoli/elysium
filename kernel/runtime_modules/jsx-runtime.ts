// User-space JSX factory, paired with kernel/transform/jsx.rs.
//
// The transform rewrites `<Tag prop={x}>child</Tag>` into `h("Tag", props,
// ...children)` calls and `<>...</>` into `h(Fragment, null, ...children)`.
// This file supplies `h` and `Fragment` for that generated code to call. It
// does no rendering of its own — it just builds a plain object tree, which a
// host-provided renderer can later walk to draw to the screen.

export type VNodeType = string | ((props: Props) => VNode);

export interface Props {
    [key: string]: unknown;
}

export interface VNode {
    type: VNodeType;
    props: Props;
    children: Child[];
}

type Child = VNode | string | number;

/** Marker type used as `h`'s `type` for `<>...</>` fragments. */
export function Fragment(props: Props): VNode {
    return { type: Fragment, props, children: props.children as Child[] };
}

export function h(type: VNodeType, props: Props | null, ...children: unknown[]): VNode {
    return {
        type,
        props: props ?? {},
        children: flattenChildren(children),
    };
}

/** JSX children arrays can nest (from `{list.map(...)}` etc.) and carry
 * holes (`null`/`undefined`/`boolean`, the common "conditionally render
 * nothing" idiom); both are cleaned up here so consumers only ever see
 * strings, numbers, and vnodes. */
function flattenChildren(children: unknown[]): Child[] {
    const out: Child[] = [];
    for (const child of children) {
        if (child === null || child === undefined || typeof child === "boolean") {
            continue;
        }
        if (Array.isArray(child)) {
            out.push(...flattenChildren(child));
        } else {
            out.push(child as Child);
        }
    }
    return out;
}