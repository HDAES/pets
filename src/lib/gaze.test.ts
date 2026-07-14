import { describe, expect, it } from "vitest";
import { gazeDirection } from "./gaze";
describe("16-way gaze", () => { it("maps cardinal directions", () => { expect(gazeDirection(0,-100,0,0)).toBe(0); expect(gazeDirection(100,0,0,0)).toBe(4); expect(gazeDirection(0,100,0,0)).toBe(8); expect(gazeDirection(-100,0,0,0)).toBe(12); }); it("uses a dead zone",()=>expect(gazeDirection(1,1,0,0)).toBeNull()); });
