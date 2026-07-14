import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalPosition } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import type { PetRecord, Settings, AnimationName } from "./lib/types";
import { SpriteRenderer } from "./lib/renderer";
import { AnimationStateMachine } from "./lib/animation";
import { gazeDirection } from "./lib/gaze";
import "./style.css";

const appWindow = getCurrentWindow();
let settings: Settings, pets: PetRecord[] = [], renderer: SpriteRenderer, state = new AnimationStateMachine();
const canvas = document.createElement("canvas");
document.querySelector("#app")!.append(canvas);

async function api<T>(cmd: string, args?: Record<string, unknown>) { return invoke<T>(cmd, args); }
async function refreshPets() { pets = await api<PetRecord[]>("list_pets"); }
async function selectPet(id: string) {
  const pet = pets.find(p => p.manifest.id === id); if (!pet) return;
  settings.currentPetId = id; await api("save_settings", { settings });
  await renderer.load(convertFileSrc(`${pet.path}/${pet.manifest.spritesheetPath}`));
}
async function applySettings(next: Partial<Settings>) {
  settings = { ...settings, ...next }; await api("save_settings", { settings });
  await appWindow.setAlwaysOnTop(settings.alwaysOnTop); await appWindow.setIgnoreCursorEvents(settings.clickThrough);
  document.body.classList.toggle("interactive", !settings.clickThrough);
}
function animate(now: number) { state.tick(now); renderer.draw(state, settings.scale); requestAnimationFrame(animate); }
function setState(name: AnimationName, direction = 0) { state.set(name, direction); }
function setupGaze() {
  window.addEventListener("mousemove", async e => {
    if (settings.clickThrough) return;
    const pos = await appWindow.outerPosition(); const scale = settings.scale;
    const d = gazeDirection(e.screenX, e.screenY, pos.x + 96 * scale, pos.y + 104 * scale, 35 * scale);
    setState(d === null ? "idle" : "gaze", d ?? 0);
  });
  canvas.addEventListener("pointerdown", async () => { if (settings.dragEnabled && !settings.clickThrough) await appWindow.startDragging(); });
  canvas.addEventListener("pointerup", () => setState("idle"));
}
function settingsPage() {
  const current = pets.find(p => p.manifest.id === settings.currentPetId);
  document.body.innerHTML = `<main><h1>桌面宠物</h1><p>当前宠物：<strong>${current?.manifest.displayName ?? "未选择"}</strong></p>
  <section><h2>宠物</h2><div id="pets"></div><button id="import">导入宠物文件夹</button><button id="data">打开宠物数据目录</button><p id="error" role="alert"></p></section>
  <section><h2>显示与交互</h2><label>缩放 <input id="scale" type="range" min="0.5" max="3" step="0.1" value="${settings.scale}"><output>${settings.scale}×</output></label>${toggle("clickThrough", "鼠标穿透")}${toggle("alwaysOnTop", "始终置顶")}${toggle("dragEnabled", "允许拖动")}${toggle("autostart", "开机自启")}</section>
  ${import.meta.env.DEV ? `<section><h2>动画调试</h2><div id="debug"></div></section>` : ""}</main>`;
  const list = document.querySelector("#pets")!;
  for (const group of ["builtin", "custom"] as const) { const groupPets = pets.filter(p => p.source === group); if (!groupPets.length) continue; const h = document.createElement("h3"); h.textContent = group === "builtin" ? "内置宠物" : "自定义宠物"; list.append(h); for (const p of groupPets) { const row = document.createElement("div"); row.className = "pet-row"; row.innerHTML = `<span>${p.manifest.displayName}</span><button>切换</button>${p.source === "custom" ? "<button>删除</button>" : ""}`; const buttons = row.querySelectorAll("button"); buttons[0].onclick = () => selectPet(p.manifest.id); if (p.source === "custom") buttons[1].onclick = async () => { await api("delete_custom_pet", { id: p.manifest.id }); await refreshPets(); settingsPage(); }; list.append(row); } }
  document.querySelectorAll<HTMLInputElement>("input[type=checkbox]").forEach(el => el.onchange = () => applySettings({ [el.id]: el.checked }));
  const scale = document.querySelector<HTMLInputElement>("#scale")!; scale.oninput = () => { applySettings({ scale: Number(scale.value) }); document.querySelector("output")!.textContent = `${scale.value}×`; };
  document.querySelector("#import")!.addEventListener("click", async () => { const path = await open({ directory: true, multiple: false, title: "选择包含 pet.json 的宠物包文件夹" }); if (!path) return; try { await api("import_pet", { sourcePath: path, conflict: "ask" }); await refreshPets(); settingsPage(); } catch (e) { const message = String(e); if (!message.includes("ID_CONFLICT:")) { document.querySelector("#error")!.textContent = message; return; } const choice = window.prompt("宠物 ID 已存在。输入 overwrite 覆盖，rename 自动重命名，或 cancel 取消：", "rename"); if (choice === "overwrite" || choice === "rename") { try { await api("import_pet", { sourcePath: path, conflict: choice }); await refreshPets(); settingsPage(); } catch (inner) { document.querySelector("#error")!.textContent = String(inner); } } } });
  document.querySelector("#data")!.addEventListener("click", () => api<string>("pet_data_dir").then(openPath));
  document.querySelector("#debug")?.append(...(["idle","running-right","running-left","waving","jumping","failed","waiting","running","review"] as AnimationName[]).map(n => { const b = document.createElement("button"); b.textContent = n; b.onclick = () => setState(n); return b; }), ...Array.from({length:16}, (_, d) => { const b=document.createElement("button"); b.textContent=`看 ${d*22.5}°`; b.onclick=()=>setState("gaze",d); return b; }));
}
function toggle(key: keyof Settings, label: string) { return `<label class="toggle">${label}<input type="checkbox" id="${key}" ${settings[key] ? "checked" : ""}></label>`; }
async function main() {
  settings = await api<Settings>("get_settings"); await refreshPets(); renderer = new SpriteRenderer(canvas); await selectPet(settings.currentPetId); await applySettings({}); if (new URLSearchParams(location.search).has("settings")) settingsPage(); else setupGaze();
  await listen("open-settings", settingsPage); await listen("reset-position", () => appWindow.setPosition(new LogicalPosition(100, 100))); await listen("debug-animation", e => { const value = e.payload as string; if (value.startsWith("gaze:")) setState("gaze", Number(value.slice(5))); else setState(value as AnimationName); });
  requestAnimationFrame(animate);
}
main().catch(console.error);
