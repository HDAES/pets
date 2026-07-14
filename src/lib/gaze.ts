/** Clockwise directions, 0=up: 000, 022.5 ... 337.5. */
export function gazeDirection(pointerX: number, pointerY: number, centerX: number, centerY: number, deadZone = 40) {
  const dx = pointerX - centerX, dy = pointerY - centerY;
  if (Math.hypot(dx, dy) < deadZone) return null;
  return Math.round((Math.atan2(dx, -dy) * 180 / Math.PI + 360) % 360 / 22.5) % 16;
}
