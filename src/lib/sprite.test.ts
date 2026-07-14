import { describe, expect, it } from "vitest";
import { spriteRect } from "./sprite";
describe("sprite coordinates", () => {
  it("maps idle frame 5 to row 0 column 5", () => expect(spriteRect("idle", 5)).toMatchObject({ sx: 960, sy: 0, count: 6 }));
  it("maps direction 8 to row 10 column 0", () => expect(spriteRect("gaze", 0, 8)).toMatchObject({ sx: 0, sy: 2080, count: 8 }));
});
