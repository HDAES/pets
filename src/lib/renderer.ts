import { spriteRect, SHEET } from "./sprite";
import type { AnimationStateMachine } from "./animation";
export class SpriteRenderer {
  private ctx: CanvasRenderingContext2D; private image = new Image();
  constructor(private canvas: HTMLCanvasElement) { const ctx = canvas.getContext("2d", { alpha: true }); if (!ctx) throw new Error("Canvas unavailable"); this.ctx = ctx; this.ctx.imageSmoothingEnabled = false; canvas.width = SHEET.cellWidth; canvas.height = SHEET.cellHeight; }
  async load(path: string) { this.image = new Image(); this.image.src = path; await this.image.decode(); }
  draw(state: AnimationStateMachine, scale: number) { const r = spriteRect(state.animation, state.frame, state.direction); this.canvas.style.width = `${SHEET.cellWidth * scale}px`; this.canvas.style.height = `${SHEET.cellHeight * scale}px`; this.ctx.clearRect(0, 0, SHEET.cellWidth, SHEET.cellHeight); this.ctx.drawImage(this.image, r.sx, r.sy, r.sw, r.sh, 0, 0, r.sw, r.sh); }
}
