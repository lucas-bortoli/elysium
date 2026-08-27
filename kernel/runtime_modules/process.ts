import type { Option } from "ely:container";

declare function __process_self_id(): number;
declare function __process_raw_arguments(): string | undefined;
declare function __process_spawn(path: string, argsJson?: string): number;
declare function __process_post_message(
  target: number,
  kind: string,
  dataJson?: string,
): void;
declare function __process_request_exit(target: number): void;
declare function __process_terminate(target: number): void;
declare function __process_is_live(id: number): boolean;
declare function __process_exit(): void;
declare function __process_set_message_handler(
  handler?: (envelope: Envelope) => void,
): void;

/** A process's identity in the kernel's table. Just a number — there is no
 * wrapper object. */
export type ProcessHandle = number;

/** A message as it arrives at `onMessage`: the sender's `kind` and payload,
 * plus who it came `from` and who it was addressed `to`. A `kind` starting
 * with `ely:` was sent by the kernel itself. */
export interface Envelope {
  kind: string;
  from: ProcessHandle;
  to: ProcessHandle;
  data: Option<unknown>;
}

/** What a caller passes to `postMessage`: a `kind` label and an optional
 * payload (anything `JSON.stringify` accepts). */
export interface Message {
  kind: string;
  data: Option<unknown>;
}

/** Thrown by `postMessage` when given a `kind` starting with `ely:`, which
 * is reserved for kernel-originated messages. */
export class ReservedMessageKindError extends Error {
  constructor(kind: string) {
    super(`message kind ${JSON.stringify(kind)} is reserved for the kernel`);
    this.name = "ReservedMessageKindError";
  }
}

/** Thrown by `postMessage`/`requestExit`/`terminate` when the target id is
 * not a live process. */
export class ProcessNotFoundError extends Error {
  constructor(id: number) {
    super(`no live process with id ${id}`);
    this.name = "ProcessNotFoundError";
  }
}

function encode(data: Option<unknown>): string | undefined {
  return data === null || data === undefined ? undefined : JSON.stringify(data);
}

function requireLive(target: ProcessHandle): void {
  if (!__process_is_live(target)) throw new ProcessNotFoundError(target);
}

/** This process's own id. */
export function currentProcessId(): ProcessHandle {
  return __process_self_id();
}

/** The argument passed to the `spawn` that started this process, or absent
 * for a process the kernel started directly (the init process). */
export function currentArguments(): Option<unknown> {
  const raw = __process_raw_arguments();
  return raw === undefined ? undefined : JSON.parse(raw);
}

/** Starts a new process from the userland-virtual entry path `path`,
 * passing `args` (structured-cloneable via JSON) as its
 * `currentArguments()`. Returns the new process's id; it joins the
 * schedule on the next frame. */
export function spawn(path: string, args: Option<unknown>): ProcessHandle {
  const json = encode(args);
  return json === undefined ? __process_spawn(path) : __process_spawn(path, json);
}

/** Queues `message` for `target`, delivered at `target`'s next turn.
 * @throws {ReservedMessageKindError} if `message.kind` starts with `ely:`.
 * @throws {ProcessNotFoundError} if `target` is not a live process. */
export function postMessage(target: ProcessHandle, message: Message): void {
  if (message.kind.startsWith("ely:")) {
    throw new ReservedMessageKindError(message.kind);
  }
  requireLive(target);
  const json = encode(message.data);
  if (json === undefined) __process_post_message(target, message.kind);
  else __process_post_message(target, message.kind, json);
}

/** Asks `target` to exit: delivers an `{ kind: "ely:exit" }` message and
 * starts a grace period after which the kernel force-reaps it. The
 * cooperative response is to clear your handlers (or call `exit()`) on
 * receiving that message.
 * @throws {ProcessNotFoundError} if `target` is not a live process. */
export function requestExit(target: ProcessHandle): void {
  requireLive(target);
  __process_request_exit(target);
}

/** Drops `target` at the end of the current frame, no grace period. Its
 * `finally` blocks do not run.
 * @throws {ProcessNotFoundError} if `target` is not a live process. */
export function terminate(target: ProcessHandle): void {
  requireLive(target);
  __process_terminate(target);
}

/** Ends this process: the kernel reaps it at the end of this turn. */
export function exit(): void {
  __process_exit();
}

/** An id returned by `addMessageHandler`, for `removeMessageHandler`. */
export type MessageHandlerId = number;

const handlers = new Map<MessageHandlerId, (envelope: Envelope) => void>();
let nextHandlerId = 1;

function dispatch(envelope: Envelope): void {
  for (const handler of [...handlers.values()]) handler(envelope);
}

/** Registers `handler` for messages sent to this process, returning an id
 * for `removeMessageHandler`. Handlers all fire, in registration order.
 * Messages that arrive before the first handler is added are queued, not
 * lost. While at least one handler is registered the process is kept
 * alive; remove them all (or call `exit()`) to let it be reaped. */
export function addMessageHandler(
  handler: (envelope: Envelope) => void,
): MessageHandlerId {
  const id = nextHandlerId++;
  const wasEmpty = handlers.size === 0;
  handlers.set(id, handler);
  if (wasEmpty) __process_set_message_handler(dispatch);
  return id;
}

/** Unregisters a handler added by `addMessageHandler`. */
export function removeMessageHandler(id: MessageHandlerId): void {
  handlers.delete(id);
  if (handlers.size === 0) __process_set_message_handler();
}
