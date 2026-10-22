/**
 * The native module ArkTS imports as `libwordcraft.so` (C++ in `src/main/cpp`).
 *
 * Coordinates are vp except a surface's width/height, which are px. Numbers for pointer kinds,
 * buttons and modifier bits match `wc_abi.h`.
 */

export const init: (settingsDir: string, tmpDir: string, fontDirs: string[], locale: string, theme: string, start: string) => number;

export const surface: (surfaceId: bigint, width: number, height: number, density: number) => number;
export const surfaceGone: () => void;
export const resize: (width: number, height: number, density: number) => void;

/** kind: 0 down, 1 up, 2 move, 3 leave, 4 cancel; button: 0 primary, 1 secondary, 2 middle. */
export const pointer: (kind: number, x: number, y: number, button: number, modifiers: number) => void;
export const scroll: (dx: number, dy: number) => void;
/** code is ArkUI's `KeyCode`; text is the key's text (`keyText`), used when it produced a character. */
export const key: (code: number, pressed: boolean, repeated: boolean, modifiers: number, text: string) => void;
export const text: (codepoint: number) => void;
export const ime: (composing: boolean) => void;
export const focus: (focused: boolean) => void;

/** 1 when a frame was presented. */
export const render: () => number;
/** Scale the page view by `factor` (a pinch; 1.0 changes nothing). */
export const zoom: (factor: number) => void;
/** Continue a scroll after the fingers lift: release velocity in vp/s, positive = scrolling down. */
export const scrollFling: (vx: number, vy: number) => void;
/** 1 while a fling is still running. */
export const flinging: () => number;

/** The input method's composing text: drawn at the caret until the commit replaces it. */
export const imePreview: (text: string) => void;
/** The input method's committed text: one undoable insert. */
export const imeCommit: (text: string) => void;

/** 1 when the document is editing text (a caret is on screen). */
export const caretPresent: () => number;
/** The caret's x in points (vp), for the input method's candidate window. */
export const caretX: () => number;
/** The caret's top y in points (vp). */
export const caretY: () => number;
/** The height of the caret's line in points (vp). */
export const caretHeight: () => number;

/** The next request from the Rust side (a JSON string), or null. Poll it once per frame. */
export const hostTake: () => string | null;
/** Answer a request: a JSON object with the result, or `{error}`, or `{cancelled: true}`. */
export const hostAnswer: (id: number, json: string) => void;

/** A JSON summary for logs (width, height, density, frames, events, paragraphs, fontDirs…). */
export const stats: () => string;
export const destroy: () => void;
