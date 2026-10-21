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
/** A JSON summary for logs (width, height, density, frames, events, paragraphs, fontDirs…). */
export const stats: () => string;
export const destroy: () => void;
