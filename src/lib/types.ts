export interface PetManifest { id: string; displayName: string; description: string; spriteVersionNumber: number; spritesheetPath: string; kind: string }
export interface PetRecord { manifest: PetManifest; source: "builtin" | "custom"; path: string }
export interface Settings { currentPetId: string; scale: number; animationSpeed: number; inputListeningEnabled: boolean; inputAnimationByPet: Record<string, AnimationName>; clickThrough: boolean; alwaysOnTop: boolean; dragEnabled: boolean; autostart: boolean; aiProvider: string; aiBaseUrl: string; aiModel: string; x?: number; y?: number }
export interface AiServiceConfig { provider: string; baseUrl: string; model: string; hasApiKey: boolean }
export type AnimationName = "idle" | "running-right" | "running-left" | "waving" | "jumping" | "failed" | "waiting" | "running" | "review" | "gaze";
