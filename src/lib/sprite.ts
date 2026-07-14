import type { AnimationName } from "./types";
export const SHEET = { width: 1536, height: 2288, cellWidth: 192, cellHeight: 208, columns: 8, rows: 11 } as const;
const frames: Record<Exclude<AnimationName, "gaze">, [number, number]> = {
  idle: [0, 6], "running-right": [1, 8], "running-left": [2, 8], waving: [3, 4], jumping: [4, 5], failed: [5, 8], waiting: [6, 6], running: [7, 6], review: [8, 6]
};
export function spriteRect(animation: AnimationName, frame: number, direction = 0) {
  const [row, count] = animation === "gaze" ? [9 + Math.floor(((direction % 16) + 16) % 16 / 8), 8] : frames[animation];
  const column = animation === "gaze" ? ((direction % 16) + 16) % 8 : frame % count;
  return { sx: column * SHEET.cellWidth, sy: row * SHEET.cellHeight, sw: SHEET.cellWidth, sh: SHEET.cellHeight, count };
}
export function frameCount(animation: AnimationName) { return animation === "gaze" ? 1 : frames[animation][1]; }
