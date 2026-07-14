import type { AnimationName } from "./types";
import { frameCount } from "./sprite";
export class AnimationStateMachine {
  animation: AnimationName = "idle"; frame = 0; direction = 0; private last = 0;
  set(animation: AnimationName, direction = 0) { if (this.animation !== animation || this.direction !== direction) { this.animation = animation; this.direction = direction; this.frame = 0; } }
  tick(now: number, frameMs = 120) { if (now - this.last >= frameMs) { this.frame = (this.frame + 1) % frameCount(this.animation); this.last = now; } }
}
