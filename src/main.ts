import { invoke } from "@tauri-apps/api/core";
import { cursorPosition, getCurrentWindow, LogicalPosition, LogicalSize } from "@tauri-apps/api/window";
import { emitTo, listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import type { PetRecord, Settings, AnimationName, AiServiceConfig } from "./lib/types";
import { SpriteRenderer } from "./lib/renderer";
import { AnimationStateMachine } from "./lib/animation";
import { gazeDirection } from "./lib/gaze";
import { SHEET } from "./lib/sprite";
import "./style.css";

const appWindow = getCurrentWindow();
const INPUT_ANIMATIONS: Array<[Exclude<AnimationName, "gaze">, string]> = [
  ["running", "专注处理"],
  ["idle", "默认待机"],
  ["waving", "挥手"],
  ["running-right", "向右跑动"],
  ["running-left", "向左跑动"],
  ["jumping", "跳跃"],
  ["waiting", "等待"],
  ["review", "检查 Review"],
  ["failed", "失败"],
];
let settings: Settings, pets: PetRecord[] = [], renderer: SpriteRenderer, state = new AnimationStateMachine();
let gazeFollowing = false;
let updateGazeNow = () => {};
let currentSpriteVersion = 2;
let petDragging = false;
let manualActionActive = false;
let inputAnimationActive = false;
let inputIdleTimer: number | undefined;
const canvas = document.createElement("canvas");
document.querySelector("#app")!.append(canvas);

async function api<T>(cmd: string, args?: Record<string, unknown>) { return invoke<T>(cmd, args); }
async function refreshPets() { pets = await api<PetRecord[]>("list_pets"); }
async function loadPet(id: string) {
  let pet = pets.find(p => p.manifest.id === id);
  if (!pet) {
    await refreshPets();
    pet = pets.find(p => p.manifest.id === id);
  }
  if (!pet) throw new Error(`找不到宠物：${id}`);
  currentSpriteVersion = pet.manifest.spriteVersionNumber;
  const bytes = await api<ArrayBuffer>("pet_spritesheet", { id });
  await renderer.load(bytes);
  gazeFollowing = false;
  setState("idle");
}
async function selectPet(id: string) {
  if (!pets.some(p => p.manifest.id === id)) return;
  settings.currentPetId = id;
  await api("save_settings", { settings });
  if (appWindow.label === "pet") await loadPet(id);
}
async function applySettings(next: Partial<Settings>) {
  settings = { ...settings, ...next }; await api("save_settings", { settings });
  await applyPetWindowSettings();
  reflectSettings();
}
async function applyPetWindowSettings() {
  if (appWindow.label !== "pet") return;
  await appWindow.setSize(new LogicalSize(SHEET.cellWidth * settings.scale, SHEET.cellHeight * settings.scale));
  await appWindow.setAlwaysOnTop(settings.alwaysOnTop);
  await appWindow.setIgnoreCursorEvents(settings.clickThrough);
}
function reflectSettings() { document.body.classList.toggle("interactive", !settings.clickThrough); }
function animate(now: number) { state.tick(now, 120 / settings.animationSpeed); renderer.draw(state, settings.scale); requestAnimationFrame(animate); }
function setState(name: AnimationName, direction = 0) { state.set(name === "gaze" && currentSpriteVersion < 2 ? "idle" : name, direction); }
function inputAnimationForPet(id: string): AnimationName { return settings.inputAnimationByPet[id] ?? "running"; }
function stopInputAnimation() {
  if (inputIdleTimer !== undefined) window.clearTimeout(inputIdleTimer);
  inputIdleTimer = undefined;
  inputAnimationActive = false;
}
function handleGlobalKeyActivity() {
  if (!settings.inputListeningEnabled || petDragging || manualActionActive) return;
  inputAnimationActive = true;
  setState(inputAnimationForPet(settings.currentPetId));
  if (inputIdleTimer !== undefined) window.clearTimeout(inputIdleTimer);
  inputIdleTimer = window.setTimeout(() => {
    inputIdleTimer = undefined;
    inputAnimationActive = false;
    setState("idle");
  }, 800);
}
async function setupGaze() {
  let isDragging = false;
  let windowPosition = await appWindow.outerPosition();
  let scaleFactor = await appWindow.scaleFactor();
  let dragEndTimer: number | undefined;
  let dragStartTimer: number | undefined;
  let pendingDrag: { pointerId: number; startX: number; startY: number; currentX: number } | undefined;
  let gazeLockedUntil = 0;
  let gazePollRunning = false;
  let restoreChatAfterDrag = false;

  const finishDrag = () => {
    if (!isDragging) return;
    isDragging = false;
    petDragging = false;
    if (dragEndTimer !== undefined) window.clearTimeout(dragEndTimer);
    dragEndTimer = undefined;
    canvas.classList.remove("dragging");
    gazeLockedUntil = performance.now() + 700;
    setState("idle");
    if (restoreChatAfterDrag) {
      restoreChatAfterDrag = false;
      void api("restore_chat_after_pet_drag");
    }
  };
  const scheduleDragEnd = () => {
    if (dragEndTimer !== undefined) window.clearTimeout(dragEndTimer);
    dragEndTimer = window.setTimeout(finishDrag, 220);
  };
  const cancelPendingDrag = () => {
    if (dragStartTimer !== undefined) window.clearTimeout(dragStartTimer);
    dragStartTimer = undefined;
    pendingDrag = undefined;
  };
  const beginDrag = async () => {
    if (!pendingDrag || isDragging) return;
    const { pointerId, currentX } = pendingDrag;
    cancelPendingDrag();
    stopInputAnimation();
    if (canvas.hasPointerCapture(pointerId)) canvas.releasePointerCapture(pointerId);
    isDragging = true;
    petDragging = true;
    canvas.classList.add("dragging");
    setState(currentX < canvas.clientWidth / 2 ? "running-left" : "running-right");
    scheduleDragEnd();
    const chatWasVisible = await api<boolean>("suspend_chat_for_pet_drag").catch(() => false);
    if (!isDragging) {
      if (chatWasVisible) void api("restore_chat_after_pet_drag");
      return;
    }
    restoreChatAfterDrag = chatWasVisible;
    void appWindow.startDragging().catch(finishDrag);
  };

  await appWindow.onMoved(({ payload: position }) => {
    const dx = position.x - windowPosition.x;
    windowPosition = position;
    if (!isDragging) return;
    if (Math.abs(dx) >= 1) setState(dx < 0 ? "running-left" : "running-right");
    scheduleDragEnd();
  });
  await appWindow.onScaleChanged(({ payload }) => { scaleFactor = payload.scaleFactor; });

  const updateGaze = async () => {
    if (!gazeFollowing || isDragging || inputAnimationActive || gazePollRunning || performance.now() < gazeLockedUntil) return;
    if (currentSpriteVersion < 2) { setState("idle"); return; }
    gazePollRunning = true;
    try {
      const cursor = await cursorPosition();
      if (!gazeFollowing || isDragging || inputAnimationActive || performance.now() < gazeLockedUntil) return;
      const width = SHEET.cellWidth * settings.scale * scaleFactor;
      const height = SHEET.cellHeight * settings.scale * scaleFactor;
      const d = gazeDirection(cursor.x, cursor.y, windowPosition.x + width / 2, windowPosition.y + height / 2, 35 * settings.scale * scaleFactor);
      setState(d === null ? "idle" : "gaze", d ?? 0);
    } finally {
      gazePollRunning = false;
    }
  };
  updateGazeNow = () => { void updateGaze(); };
  window.setInterval(() => { void updateGaze(); }, 100);

  canvas.addEventListener("pointerdown", event => {
    if (!settings.dragEnabled || settings.clickThrough || event.button !== 0) return;
    pendingDrag = { pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, currentX: event.clientX };
    canvas.setPointerCapture(event.pointerId);
    dragStartTimer = window.setTimeout(beginDrag, 160);
  });
  canvas.addEventListener("pointermove", event => {
    if (!pendingDrag || pendingDrag.pointerId !== event.pointerId) return;
    pendingDrag.currentX = event.clientX;
    if (Math.hypot(event.clientX - pendingDrag.startX, event.clientY - pendingDrag.startY) >= 4) void beginDrag();
  });
  canvas.addEventListener("click", event => {
    if (event.detail !== 1) return;
    stopInputAnimation();
    manualActionActive = false;
    gazeFollowing = false;
    setState("idle");
  });
  canvas.addEventListener("dblclick", event => {
    event.preventDefault();
    finishDrag();
    stopInputAnimation();
    manualActionActive = false;
    gazeFollowing = false;
    setState("idle");
    void api("toggle_chat");
  });
  canvas.addEventListener("contextmenu", event => {
    event.preventDefault();
    finishDrag();
    stopInputAnimation();
    manualActionActive = false;
    gazeFollowing = false;
    setState("idle");
    void api("open_action_menu");
  });
  window.addEventListener("pointerup", () => { cancelPendingDrag(); finishDrag(); }, true);
  window.addEventListener("pointercancel", () => { cancelPendingDrag(); finishDrag(); }, true);
}
function actionMenuPage() {
  const actions: Array<[string, string, string]> = [
    ["chat", "和我聊天", "✦"],
    ["follow", "鼠标跟随", "◎"],
    ["idle", "默认待机", "◌"],
    ["waving", "挥手", "◇"],
    ["running-right", "向右跑动", "→"],
    ["running-left", "向左跑动", "←"],
    ["jumping", "跳跃", "↑"],
    ["waiting", "等待", "…"],
    ["running", "专注处理", "●"],
    ["review", "检查 Review", "✓"],
    ["failed", "失败", "!"],
  ];
  document.body.className = "actions-page interactive";
  document.body.innerHTML = `<aside class="action-menu"><header><span class="action-mark">✦</span><div><h1>动作面板</h1><p>选择动作 · 点击外部关闭</p></div></header><div id="actions"></div><footer><button id="close-panel" type="button">关闭面板</button><button id="quit-app" class="danger" type="button">退出桌宠</button></footer></aside>`;
  const list = document.querySelector("#actions")!;
  for (const [value, label, icon] of actions) {
    const button = document.createElement("button");
    button.dataset.action = value;
    button.innerHTML = `<span class="action-icon">${icon}</span><span>${label}</span>${value === "chat" ? `<small>打开宠物聊天面板</small>` : value === "follow" ? `<small>视线持续跟随光标</small>` : ""}`;
    button.onclick = async () => {
      if (value === "chat") {
        await api("open_chat");
        await appWindow.hide();
        return;
      }
      list.querySelectorAll("button").forEach(item => { item.classList.remove("selected"); item.setAttribute("aria-pressed", "false"); });
      button.classList.add("selected");
      button.setAttribute("aria-pressed", "true");
      await emitTo("pet", "pet-action", value);
    };
    button.setAttribute("aria-pressed", "false");
    list.append(button);
  }
  document.querySelector<HTMLButtonElement>("#close-panel")!.onclick = () => { void appWindow.hide(); };
  document.querySelector<HTMLButtonElement>("#quit-app")!.onclick = () => { void api("quit_app"); };
}
async function chatPage() {
  settings = await api<Settings>("get_settings");
  await refreshPets();
  const current = pets.find(p => p.manifest.id === settings.currentPetId);
  const name = current?.manifest.displayName ?? "宠物";
  document.body.className = "chat-page interactive";
  document.body.innerHTML = `<main class="chat-panel">
    <header>
      <span class="chat-avatar" data-tauri-drag-region>✦</span>
      <div data-tauri-drag-region><h1 id="chat-pet-name"></h1><p>AI 聊天</p></div>
      <button id="close-chat" type="button" aria-label="关闭聊天">×</button>
    </header>
    <section class="chat-messages" aria-live="polite">
      <div class="chat-empty"><span>◇</span><strong id="chat-empty-title"></strong><p>在设置中配置 AI 服务后，就可以开始聊天。</p></div>
    </section>
    <footer class="chat-composer">
      <textarea id="chat-input" rows="2" placeholder="输入消息…"></textarea>
      <button id="chat-send" type="button">发送</button>
    </footer>
  </main>`;
  const updatePetName = (nextName: string) => {
    document.querySelector("#chat-pet-name")!.textContent = nextName;
    const emptyTitle = document.querySelector("#chat-empty-title");
    if (emptyTitle) emptyTitle.textContent = `和 ${nextName} 聊聊天`;
  };
  updatePetName(name);
  const closeButton = document.querySelector<HTMLButtonElement>("#close-chat")!;
  const closeChat = (event: Event) => {
    event.preventDefault();
    event.stopPropagation();
    void api("hide_chat");
  };
  closeButton.onpointerdown = closeChat;
  closeButton.onclick = closeChat;
  const messageList = document.querySelector<HTMLElement>(".chat-messages")!;
  const input = document.querySelector<HTMLTextAreaElement>("#chat-input")!;
  const send = document.querySelector<HTMLButtonElement>("#chat-send")!;
  type ChatMessage = { role: "user" | "assistant"; content: string };
  const histories = new Map<string, ChatMessage[]>();
  const drafts = new Map<string, string>();
  const historyFor = (petId: string) => {
    let history = histories.get(petId);
    if (!history) {
      history = [];
      histories.set(petId, history);
    }
    return history;
  };
  let activeRequestId: string | undefined;
  let activeRequestPetId: string | undefined;
  let activeReplyMessage: ChatMessage | undefined;
  let activeReply: HTMLElement | undefined;
  const appendMessage = (role: "user" | "assistant", content: string) => {
    messageList.querySelector(".chat-empty")?.remove();
    const item = document.createElement("div");
    item.className = `chat-message ${role}`;
    item.textContent = content;
    messageList.append(item);
    messageList.scrollTop = messageList.scrollHeight;
    return item;
  };
  const renderHistory = (petId: string, petName: string) => {
    messageList.replaceChildren();
    const history = historyFor(petId);
    if (!history.length) {
      const empty = document.createElement("div");
      empty.className = "chat-empty";
      empty.innerHTML = "<span>◇</span><strong></strong><p>在设置中配置 AI 服务后，就可以开始聊天。</p>";
      empty.querySelector("strong")!.textContent = `和 ${petName} 聊聊天`;
      messageList.append(empty);
      return;
    }
    for (const message of history) {
      const item = appendMessage(message.role, message.content);
      if (message === activeReplyMessage && petId === activeRequestPetId) activeReply = item;
    }
  };
  const refreshAvailability = async () => {
    const config = await api<AiServiceConfig>("get_ai_service_config");
    const ready = config.hasApiKey && Boolean(config.model.trim());
    input.disabled = !ready;
    send.disabled = !ready;
    input.placeholder = ready ? "输入消息…" : "请先在设置中配置 AI 服务和 API Key";
  };
  await listen<{ requestId: string; delta: string }>("chat-stream-delta", event => {
    if (event.payload.requestId !== activeRequestId || !activeReplyMessage) return;
    activeReplyMessage.content += event.payload.delta;
    if (activeRequestPetId === settings.currentPetId && activeReply) {
      activeReply.textContent = activeReplyMessage.content;
      messageList.scrollTop = messageList.scrollHeight;
    }
  });
  const finishRequest = async (requestId: string) => {
    if (activeRequestId !== requestId) return;
    activeRequestId = undefined;
    activeRequestPetId = undefined;
    activeReplyMessage = undefined;
    activeReply = undefined;
    send.textContent = "发送";
    await refreshAvailability();
    if (!input.disabled) input.focus();
  };
  const submit = async () => {
    if (activeRequestId) {
      await api("cancel_chat_request", { requestId: activeRequestId });
      return;
    }
    const content = input.value.trim();
    if (!content) return;
    const requestPetId = settings.currentPetId;
    const history = historyFor(requestPetId);
    input.value = "";
    drafts.set(requestPetId, "");
    history.push({ role: "user", content });
    appendMessage("user", content);
    const replyMessage: ChatMessage = { role: "assistant", content: "" };
    activeReplyMessage = replyMessage;
    history.push(replyMessage);
    activeReply = appendMessage("assistant", "");
    activeRequestId = crypto.randomUUID();
    activeRequestPetId = requestPetId;
    const requestId = activeRequestId;
    send.textContent = "停止";
    input.disabled = true;
    await emitTo("pet", "chat-status", "thinking");
    try {
      await api("send_chat_message", { requestId, messages: history.slice(0, -1) });
      if (!replyMessage.content) replyMessage.content = "已停止生成。";
      if (settings.currentPetId === requestPetId) await emitTo("pet", "chat-status", "completed");
    } catch (error) {
      replyMessage.content = `请求失败：${String(error)}`;
      if (settings.currentPetId === requestPetId) await emitTo("pet", "chat-status", "failed");
    } finally {
      if (settings.currentPetId === requestPetId) {
        renderHistory(requestPetId, pets.find(p => p.manifest.id === requestPetId)?.manifest.displayName ?? "宠物");
      }
      await finishRequest(requestId);
    }
  };
  send.onclick = () => { void submit(); };
  input.addEventListener("keydown", event => {
    if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); void submit(); }
  });
  window.addEventListener("keydown", event => { if (event.key === "Escape") void api("hide_chat"); });
  await listen("settings-changed", async event => {
    const previousPetId = settings.currentPetId;
    drafts.set(previousPetId, input.value);
    settings = event.payload as Settings;
    await refreshPets();
    const petName = pets.find(p => p.manifest.id === settings.currentPetId)?.manifest.displayName ?? "宠物";
    updatePetName(petName);
    if (previousPetId !== settings.currentPetId) {
      if (activeRequestId && activeRequestPetId === previousPetId) {
        const requestId = activeRequestId;
        if (activeReplyMessage && !activeReplyMessage.content) activeReplyMessage.content = "已停止生成。";
        activeRequestId = undefined;
        activeRequestPetId = undefined;
        activeReplyMessage = undefined;
        activeReply = undefined;
        send.textContent = "发送";
        void api("cancel_chat_request", { requestId });
      }
      renderHistory(settings.currentPetId, petName);
      input.value = drafts.get(settings.currentPetId) ?? "";
      await refreshAvailability();
    }
  });
  await listen("ai-config-changed", () => { void refreshAvailability(); });
  await refreshAvailability();
  renderHistory(settings.currentPetId, name);
}
async function aiServiceSettingsPanel() {
  const panel = document.querySelector<HTMLDivElement>("#ai-service-config");
  if (!panel) return;
  panel.innerHTML = `<p class="ai-config-loading">正在读取 AI 配置…</p>`;
  let config: AiServiceConfig;
  try {
    config = await api<AiServiceConfig>("get_ai_service_config");
  } catch (error) {
    panel.innerHTML = `<p class="ai-config-status error" role="alert"></p>`;
    panel.querySelector("p")!.textContent = String(error);
    return;
  }
  panel.innerHTML = `<div class="ai-config-grid">
    <label>服务类型<select id="aiProvider"><option value="openai-compatible">DeepSeek / OpenAI 兼容接口</option></select></label>
    <label>API 地址<input id="aiBaseUrl" type="url" spellcheck="false" autocomplete="off" placeholder="https://api.example.com/v1"></label>
    <label>模型名称<input id="aiModel" type="text" spellcheck="false" autocomplete="off" placeholder="deepseek-v4-flash"></label>
    <label>API Key<input id="aiApiKey" type="password" spellcheck="false" autocomplete="off"></label>
  </div>
  <p class="ai-key-hint">API Key 保存在系统安全凭据存储中，应用设置和日志不会记录明文。</p>
  <div class="ai-config-actions"><button id="save-ai-config" type="button">保存 AI 配置</button><button id="delete-ai-key" class="secondary danger" type="button">删除 API Key</button></div>
  <p id="ai-config-status" class="ai-config-status" role="status" aria-live="polite"></p>`;
  const provider = panel.querySelector<HTMLSelectElement>("#aiProvider")!;
  const baseUrl = panel.querySelector<HTMLInputElement>("#aiBaseUrl")!;
  const model = panel.querySelector<HTMLInputElement>("#aiModel")!;
  const apiKey = panel.querySelector<HTMLInputElement>("#aiApiKey")!;
  const save = panel.querySelector<HTMLButtonElement>("#save-ai-config")!;
  const remove = panel.querySelector<HTMLButtonElement>("#delete-ai-key")!;
  const status = panel.querySelector<HTMLParagraphElement>("#ai-config-status")!;
  provider.value = config.provider;
  baseUrl.value = config.baseUrl;
  model.value = config.model;
  const reflectKeyStatus = (hasApiKey: boolean) => {
    apiKey.value = "";
    apiKey.placeholder = hasApiKey ? "已安全保存；留空表示不更换" : "输入 API Key";
    remove.disabled = !hasApiKey;
  };
  reflectKeyStatus(config.hasApiKey);
  save.onclick = async () => {
    status.className = "ai-config-status";
    status.textContent = "正在保存…";
    save.disabled = true;
    try {
      const next = await api<AiServiceConfig>("save_ai_service_config", {
        config: { provider: provider.value, baseUrl: baseUrl.value, model: model.value },
        apiKey: apiKey.value.trim() || null,
      });
      baseUrl.value = next.baseUrl;
      model.value = next.model;
      reflectKeyStatus(next.hasApiKey);
      status.textContent = next.hasApiKey ? "AI 配置已保存，API Key 已安全存储。" : "服务配置已保存，请继续填写 API Key。";
    } catch (error) {
      status.className = "ai-config-status error";
      status.textContent = String(error);
    } finally {
      save.disabled = false;
    }
  };
  remove.onclick = async () => {
    if (!window.confirm("确定从系统安全凭据存储中删除 API Key？")) return;
    remove.disabled = true;
    status.className = "ai-config-status";
    status.textContent = "正在删除…";
    try {
      await api("delete_ai_api_key");
      reflectKeyStatus(false);
      status.textContent = "API Key 已删除。";
    } catch (error) {
      remove.disabled = false;
      status.className = "ai-config-status error";
      status.textContent = String(error);
    }
  };
}
function settingsPage() {
  document.body.className = "settings-page interactive";
  const current = pets.find(p => p.manifest.id === settings.currentPetId);
  const inputAnimation = inputAnimationForPet(settings.currentPetId);
  const inputAnimationOptions = INPUT_ANIMATIONS.map(([value, label]) => `<option value="${value}" ${value === inputAnimation ? "selected" : ""}>${label} · ${value}</option>`).join("");
  document.body.innerHTML = `<main><h1>桌面宠物</h1><p>当前宠物：<strong>${current?.manifest.displayName ?? "未选择"}</strong></p>
  <section><h2>宠物</h2><div id="pets"></div><button id="import">导入宠物文件夹</button><button id="data">打开宠物数据目录</button><p id="error" role="alert"></p></section>
  <section><h2>AI 服务</h2><p>配置你自己的 AI 接口，聊天内容将直接发送到该服务。</p><div id="ai-service-config"></div></section>
  <section><h2>显示与交互</h2><label>缩放 <input id="scale" type="range" min="0.5" max="3" step="0.1" value="${settings.scale}"><output id="scale-output">${settings.scale}×</output></label><label>动画速度 <input id="animationSpeed" type="range" min="0.25" max="2" step="0.05" value="${settings.animationSpeed}"><output id="speed-output">${settings.animationSpeed.toFixed(2)}×</output></label>${toggle("inputListeningEnabled", "输入动作监听")}<label class="select-setting">当前宠物的输入动作<select id="inputAnimation" ${settings.inputListeningEnabled ? "" : "disabled"}>${inputAnimationOptions}</select></label>${toggle("clickThrough", "鼠标穿透")}${toggle("alwaysOnTop", "始终置顶")}${toggle("dragEnabled", "允许拖动")}${toggle("autostart", "开机自启")}</section>
  ${import.meta.env.DEV ? `<section><h2>动画调试</h2><div id="debug"></div></section>` : ""}</main>`;
  const list = document.querySelector("#pets")!;
  for (const group of ["builtin", "custom"] as const) { const groupPets = pets.filter(p => p.source === group); if (!groupPets.length) continue; const h = document.createElement("h3"); h.textContent = group === "builtin" ? "内置宠物" : "自定义宠物"; list.append(h); for (const p of groupPets) { const row = document.createElement("div"); row.className = "pet-row"; row.innerHTML = `<span>${p.manifest.displayName}</span><button>切换</button>${p.source === "custom" ? "<button>删除</button>" : ""}`; const buttons = row.querySelectorAll("button"); buttons[0].onclick = async () => { await selectPet(p.manifest.id); settingsPage(); }; if (p.source === "custom") buttons[1].onclick = async () => { await api("delete_custom_pet", { id: p.manifest.id }); await refreshPets(); settingsPage(); }; list.append(row); } }
  document.querySelectorAll<HTMLInputElement>("input[type=checkbox]").forEach(el => el.onchange = () => applySettings({ [el.id]: el.checked }));
  const scale = document.querySelector<HTMLInputElement>("#scale")!; scale.oninput = () => { applySettings({ scale: Number(scale.value) }); document.querySelector("#scale-output")!.textContent = `${scale.value}×`; };
  const animationSpeed = document.querySelector<HTMLInputElement>("#animationSpeed")!; animationSpeed.oninput = () => { const value = Number(animationSpeed.value); applySettings({ animationSpeed: value }); document.querySelector("#speed-output")!.textContent = `${value.toFixed(2)}×`; };
  const inputAnimationSelect = document.querySelector<HTMLSelectElement>("#inputAnimation")!; inputAnimationSelect.onchange = () => { const animation = inputAnimationSelect.value as AnimationName; applySettings({ inputAnimationByPet: { ...settings.inputAnimationByPet, [settings.currentPetId]: animation } }); };
  const inputListeningToggle = document.querySelector<HTMLInputElement>("#inputListeningEnabled")!; inputListeningToggle.onchange = () => { inputAnimationSelect.disabled = !inputListeningToggle.checked; applySettings({ inputListeningEnabled: inputListeningToggle.checked }); };
  document.querySelector("#import")!.addEventListener("click", async () => { const path = await open({ directory: true, multiple: false, title: "选择包含 pet.json 的宠物包文件夹" }); if (!path) return; try { await api("import_pet", { sourcePath: path, conflict: "ask" }); await refreshPets(); settingsPage(); } catch (e) { const message = String(e); if (!message.includes("ID_CONFLICT:")) { document.querySelector("#error")!.textContent = message; return; } const choice = window.prompt("宠物 ID 已存在。输入 overwrite 覆盖，rename 自动重命名，或 cancel 取消：", "rename"); if (choice === "overwrite" || choice === "rename") { try { await api("import_pet", { sourcePath: path, conflict: choice }); await refreshPets(); settingsPage(); } catch (inner) { document.querySelector("#error")!.textContent = String(inner); } } } });
  document.querySelector("#data")!.addEventListener("click", () => api<string>("pet_data_dir").then(openPath));
  const previewAnimation = (value: string) => { void emitTo("pet", "debug-animation", value); };
  document.querySelector("#debug")?.append(...(["idle","running-right","running-left","waving","jumping","failed","waiting","running","review"] as AnimationName[]).map(n => { const b = document.createElement("button"); b.textContent = n; b.onclick = () => previewAnimation(n); return b; }), ...Array.from({length:16}, (_, d) => { const b=document.createElement("button"); b.textContent=`看 ${d*22.5}°`; b.onclick=()=>previewAnimation(`gaze:${d}`); return b; }));
  void aiServiceSettingsPanel();
}
function toggle(key: keyof Settings, label: string) { return `<label class="toggle">${label}<input type="checkbox" id="${key}" ${settings[key] ? "checked" : ""}></label>`; }
async function main() {
  const params = new URLSearchParams(location.search);
  if (appWindow.label === "actions" || params.has("actions")) { actionMenuPage(); return; }
  if (appWindow.label === "chat" || params.has("chat")) { await chatPage(); return; }
  settings = await api<Settings>("get_settings"); await refreshPets(); renderer = new SpriteRenderer(canvas); await loadPet(settings.currentPetId); await applySettings({}); if (appWindow.label === "settings" || params.has("settings")) settingsPage(); else await setupGaze();
  await listen("settings-changed", async event => { const previousPetId = settings.currentPetId; settings = event.payload as Settings; if (appWindow.label === "pet" && previousPetId !== settings.currentPetId) await loadPet(settings.currentPetId); if (appWindow.label === "pet" && !settings.inputListeningEnabled && inputAnimationActive) { stopInputAnimation(); setState("idle"); } else if (appWindow.label === "pet" && inputAnimationActive) { setState(inputAnimationForPet(settings.currentPetId)); } await applyPetWindowSettings(); reflectSettings(); });
  await listen("pet-action", event => { const value = event.payload as string; stopInputAnimation(); if (value === "follow") { manualActionActive = false; gazeFollowing = true; updateGazeNow(); } else { manualActionActive = value !== "idle"; gazeFollowing = false; setState(value as AnimationName); } });
  await listen("chat-status", event => {
    const status = event.payload as string;
    stopInputAnimation();
    gazeFollowing = false;
    manualActionActive = false;
    setState(status === "thinking" ? "running" : status === "completed" ? "review" : "failed");
    if (status !== "thinking") window.setTimeout(() => setState("idle"), 900);
  });
  if (appWindow.label === "pet") await listen("global-key-activity", handleGlobalKeyActivity);
  await listen("open-settings", settingsPage); await listen("reset-position", () => appWindow.setPosition(new LogicalPosition(100, 100))); await listen("debug-animation", e => { gazeFollowing = false; const value = e.payload as string; if (value.startsWith("gaze:")) setState("gaze", Number(value.slice(5))); else setState(value as AnimationName); });
  requestAnimationFrame(animate);
}
main().catch(async error => {
  console.error(error);
  document.body.classList.add("startup-failed", "interactive");
  document.body.innerHTML = `<div class="startup-error"><strong>桌面宠物启动失败</strong><span>${String(error)}</span><small>请通过系统托盘退出后重新启动。</small></div>`;
  await appWindow.setIgnoreCursorEvents(false).catch(() => undefined);
});
