export interface PetManifest { id: string; displayName: string; description: string; spriteVersionNumber: number; spritesheetPath: string; kind: string }
export interface PetRecord { manifest: PetManifest; source: "builtin" | "custom"; path: string }
export interface Settings { currentPetId: string; scale: number; clickThrough: boolean; alwaysOnTop: boolean; dragEnabled: boolean; autostart: boolean; x?: number; y?: number }
export type AnimationName = "idle" | "running-right" | "running-left" | "waving" | "jumping" | "failed" | "waiting" | "running" | "review" | "gaze";
