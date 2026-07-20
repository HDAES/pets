use image::GenericImageView;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::ManagerExt;

const W: u32 = 1536;
const H_V1: u32 = 1872;
const H_V2: u32 = 2288;
const BUILTIN_MANIFEST: &str = include_str!("../resources/builtin/pet.json");
const BUILTIN_SPRITESHEET: &[u8] = include_bytes!("../resources/builtin/spritesheet.webp");
fn default_sprite_version() -> u32 {
    1
}
#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PetManifest {
    pub id: String,
    pub display_name: String,
    pub description: String,
    #[serde(default = "default_sprite_version")]
    pub sprite_version_number: u32,
    pub spritesheet_path: String,
    pub kind: String,
}
#[derive(Clone, Serialize)]
pub struct PetRecord {
    manifest: PetManifest,
    source: String,
    path: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    current_pet_id: String,
    scale: f64,
    #[serde(default = "default_animation_speed")]
    animation_speed: f64,
    #[serde(default = "default_input_listening_enabled")]
    input_listening_enabled: bool,
    #[serde(default)]
    input_animation_by_pet: BTreeMap<String, String>,
    click_through: bool,
    always_on_top: bool,
    drag_enabled: bool,
    autostart: bool,
    #[serde(default = "default_ai_provider")]
    ai_provider: String,
    #[serde(default = "default_ai_base_url")]
    ai_base_url: String,
    #[serde(default = "default_ai_model")]
    ai_model: String,
    x: Option<i32>,
    y: Option<i32>,
}
fn default_animation_speed() -> f64 {
    1.0
}
fn default_input_listening_enabled() -> bool {
    true
}
fn default_ai_provider() -> String {
    "openai-compatible".into()
}
fn default_ai_base_url() -> String {
    "https://api.deepseek.com".into()
}
fn default_ai_model() -> String {
    "deepseek-v4-flash".into()
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            current_pet_id: "ikunchick".into(),
            scale: 1.0,
            animation_speed: default_animation_speed(),
            input_listening_enabled: default_input_listening_enabled(),
            input_animation_by_pet: BTreeMap::new(),
            click_through: true,
            always_on_top: true,
            drag_enabled: true,
            autostart: true,
            ai_provider: default_ai_provider(),
            ai_base_url: default_ai_base_url(),
            ai_model: default_ai_model(),
            x: None,
            y: None,
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiServiceConfig {
    provider: String,
    base_url: String,
    model: String,
    has_api_key: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiServiceConfigInput {
    provider: String,
    base_url: String,
    model: String,
}
pub struct AppState {
    settings: Mutex<Settings>,
    cancelled_chat_requests: Mutex<HashSet<String>>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    role: String,
    content: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatDelta {
    request_id: String,
    delta: String,
}

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}
fn pets_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("pets"))
}
fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("settings.json"))
}
fn write_settings(app: &AppHandle, s: &Settings) -> Result<(), String> {
    let p = config_path(app)?;
    fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(p, serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn read_settings(app: &AppHandle) -> Settings {
    let mut settings: Settings = config_path(app)
        .ok()
        .and_then(|p| fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    if settings.ai_base_url == "https://api.openai.com/v1" && settings.ai_model.trim().is_empty() {
        settings.ai_base_url = default_ai_base_url();
        settings.ai_model = default_ai_model();
    }
    settings
}

const KEYRING_SERVICE: &str = "com.petdesk.app";
const KEYRING_AI_API_KEY: &str = "ai-api-key";

fn ai_api_key_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_AI_API_KEY)
        .map_err(|e| format!("无法访问系统凭据存储：{e}"))
}
fn has_ai_api_key() -> Result<bool, String> {
    match ai_api_key_entry()?.get_password() {
        Ok(value) => Ok(!value.is_empty()),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(e) => Err(format!("无法读取 API Key 状态：{e}")),
    }
}
fn validate_ai_service_config(config: &AiServiceConfigInput) -> Result<(), String> {
    if config.provider != "openai-compatible" {
        return Err("当前仅支持 OpenAI 兼容接口".into());
    }
    if config.model.trim().is_empty() || config.model.trim().len() > 120 {
        return Err("模型名称不能为空且不能超过 120 个字符".into());
    }
    let url = url::Url::parse(config.base_url.trim()).map_err(|_| "API 地址格式无效")?;
    let local_http =
        url.scheme() == "http" && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if url.scheme() != "https" && !local_http {
        return Err("API 地址必须使用 HTTPS；本机 localhost 服务可以使用 HTTP".into());
    }
    if url.host_str().is_none() {
        return Err("API 地址必须包含主机名".into());
    }
    Ok(())
}
fn builtin_manifest() -> Result<PetManifest, String> {
    serde_json::from_str(BUILTIN_MANIFEST).map_err(|e| format!("内置宠物 manifest 无效：{e}"))
}

pub fn validate_package(dir: &Path) -> Result<PetManifest, String> {
    let manifest_path = dir.join("pet.json");
    let bytes = fs::read(&manifest_path)
        .map_err(|_| format!("找不到 pet.json：{}", manifest_path.display()))?;
    let m: PetManifest =
        serde_json::from_slice(&bytes).map_err(|e| format!("pet.json 不是合法 JSON：{e}"))?;
    if m.id.trim().is_empty()
        || !m
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("pet.json 的 id 必须为非空 ASCII 字母、数字、- 或 _".into());
    }
    if m.display_name.trim().is_empty() || m.kind.trim().is_empty() {
        return Err("pet.json 必须包含 displayName 和 kind".into());
    }
    if !matches!(m.sprite_version_number, 1 | 2) {
        return Err(format!(
            "当前仅支持 spriteVersionNumber: 1 或 2，收到 {}",
            m.sprite_version_number
        ));
    }
    let sheet = dir.join(&m.spritesheet_path);
    if sheet
        .extension()
        .and_then(|x| x.to_str())
        .map(|x| x.eq_ignore_ascii_case("webp"))
        != Some(true)
    {
        return Err("v1 仅支持 WebP spritesheet".into());
    }
    let reader = image::ImageReader::open(&sheet)
        .map_err(|_| format!("找不到精灵图：{}", sheet.display()))?
        .with_guessed_format()
        .map_err(|e| format!("不能识别精灵图格式：{e}"))?;
    if reader.format() != Some(image::ImageFormat::WebP) {
        return Err("精灵图内容必须是 WebP 格式".into());
    }
    let img = reader
        .decode()
        .map_err(|e| format!("无法解码 WebP 精灵图：{e}"))?;
    validate_sheet_properties(
        img.dimensions(),
        img.color().has_alpha(),
        m.sprite_version_number,
    )?;
    Ok(m)
}
fn validate_sheet_properties(
    dimensions: (u32, u32),
    has_alpha: bool,
    version: u32,
) -> Result<(), String> {
    let expected_height = match version {
        1 => H_V1,
        2 => H_V2,
        _ => return Err(format!("不支持 spriteVersionNumber: {version}")),
    };
    if dimensions != (W, expected_height) {
        return Err(format!(
            "v{version} 精灵图尺寸必须为 {W}×{expected_height}，实际为 {}×{}",
            dimensions.0, dimensions.1
        ));
    }
    if !has_alpha {
        return Err("精灵图必须包含透明通道".into());
    }
    Ok(())
}
fn custom_record(dir: PathBuf) -> Result<PetRecord, String> {
    let m = validate_package(&dir)?;
    Ok(PetRecord {
        manifest: m,
        source: "custom".into(),
        path: dir.to_string_lossy().into_owned(),
    })
}
#[tauri::command]
fn list_pets(app: AppHandle) -> Result<Vec<PetRecord>, String> {
    let mut pets = vec![PetRecord {
        manifest: builtin_manifest()?,
        source: "builtin".into(),
        path: "builtin://ikunchick".into(),
    }];
    let dir = pets_dir(&app)?;
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                if let Ok(p) = custom_record(e.path()) {
                    pets.push(p)
                }
            }
        }
    };
    Ok(pets)
}
#[tauri::command]
fn pet_spritesheet(app: AppHandle, id: String) -> Result<tauri::ipc::Response, String> {
    if id == "ikunchick" {
        return Ok(tauri::ipc::Response::new(BUILTIN_SPRITESHEET.to_vec()));
    }
    if id.trim().is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("宠物 id 无效".into());
    }
    let dir = pets_dir(&app)?.join(&id);
    let manifest = validate_package(&dir)?;
    if manifest.id != id {
        return Err("宠物目录与 manifest id 不一致".into());
    }
    let bytes = fs::read(dir.join(manifest.spritesheet_path))
        .map_err(|e| format!("读取宠物图集失败：{e}"))?;
    Ok(tauri::ipc::Response::new(bytes))
}
#[tauri::command]
fn get_settings(state: tauri::State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}
#[tauri::command]
fn get_ai_service_config(state: tauri::State<AppState>) -> Result<AiServiceConfig, String> {
    let (provider, base_url, model) = {
        let settings = state.settings.lock().unwrap();
        (
            settings.ai_provider.clone(),
            settings.ai_base_url.clone(),
            settings.ai_model.clone(),
        )
    };
    Ok(AiServiceConfig {
        provider,
        base_url,
        model,
        has_api_key: has_ai_api_key()?,
    })
}
#[tauri::command]
fn save_settings(
    app: AppHandle,
    state: tauri::State<AppState>,
    settings: Settings,
) -> Result<(), String> {
    if !(0.25..=2.0).contains(&settings.animation_speed) {
        return Err("动画速度必须在 0.25× 到 2.0× 之间".into());
    }
    const STANDARD_ANIMATIONS: [&str; 9] = [
        "idle",
        "running-right",
        "running-left",
        "waving",
        "jumping",
        "failed",
        "waiting",
        "running",
        "review",
    ];
    if settings
        .input_animation_by_pet
        .values()
        .any(|animation| !STANDARD_ANIMATIONS.contains(&animation.as_str()))
    {
        return Err("输入监听动作必须是标准动画".into());
    }
    if let Some(w) = app.get_webview_window("pet") {
        w.set_always_on_top(settings.always_on_top)
            .map_err(|e| e.to_string())?;
        w.set_ignore_cursor_events(settings.click_through)
            .map_err(|e| e.to_string())?;
    }
    if settings.autostart {
        let _ = app.autolaunch().enable();
    } else {
        let _ = app.autolaunch().disable();
    }
    write_settings(&app, &settings)?;
    *state.settings.lock().unwrap() = settings.clone();
    let _ = app.emit("settings-changed", &settings);
    Ok(())
}
#[tauri::command]
fn save_ai_service_config(
    app: AppHandle,
    state: tauri::State<AppState>,
    config: AiServiceConfigInput,
    api_key: Option<String>,
) -> Result<AiServiceConfig, String> {
    validate_ai_service_config(&config)?;
    if let Some(api_key) = api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    {
        if api_key.len() > 4096 {
            return Err("API Key 长度不能超过 4096 个字符".into());
        }
        ai_api_key_entry()?
            .set_password(api_key)
            .map_err(|e| format!("无法将 API Key 写入系统凭据存储：{e}"))?;
    }
    let mut settings = state.settings.lock().unwrap().clone();
    settings.ai_provider = config.provider;
    settings.ai_base_url = config.base_url.trim().trim_end_matches('/').into();
    settings.ai_model = config.model.trim().into();
    save_settings(app.clone(), state, settings.clone())?;
    let _ = app.emit("ai-config-changed", ());
    Ok(AiServiceConfig {
        provider: settings.ai_provider,
        base_url: settings.ai_base_url,
        model: settings.ai_model,
        has_api_key: has_ai_api_key()?,
    })
}
#[tauri::command]
fn delete_ai_api_key(app: AppHandle) -> Result<(), String> {
    match ai_api_key_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            let _ = app.emit("ai-config-changed", ());
            Ok(())
        }
        Err(e) => Err(format!("无法从系统凭据存储删除 API Key：{e}")),
    }
}
fn get_ai_api_key() -> Result<String, String> {
    match ai_api_key_entry()?.get_password() {
        Ok(value) if !value.trim().is_empty() => Ok(value),
        Ok(_) | Err(keyring::Error::NoEntry) => Err("尚未配置 API Key".into()),
        Err(e) => Err(format!("无法从系统凭据存储读取 API Key：{e}")),
    }
}

fn parse_chat_sse_event(event: &str) -> Result<(Vec<String>, bool), String> {
    let mut deltas = Vec::new();
    for line in event.lines() {
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if data == "[DONE]" {
            return Ok((deltas, true));
        }
        if data.is_empty() {
            continue;
        }
        let value: serde_json::Value =
            serde_json::from_str(data).map_err(|_| "AI 服务返回了无法解析的流数据")?;
        if let Some(delta) = value
            .pointer("/choices/0/delta/content")
            .and_then(|value| value.as_str())
            .filter(|delta| !delta.is_empty())
        {
            deltas.push(delta.to_owned());
        }
    }
    Ok((deltas, false))
}

#[tauri::command]
fn cancel_chat_request(state: tauri::State<AppState>, request_id: String) {
    state
        .cancelled_chat_requests
        .lock()
        .unwrap()
        .insert(request_id);
}

#[tauri::command]
async fn send_chat_message(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request_id: String,
    messages: Vec<ChatMessage>,
) -> Result<(), String> {
    if messages.is_empty() || messages.len() > 40 {
        return Err("聊天上下文必须包含 1–40 条消息".into());
    }
    if messages.iter().any(|message| {
        !matches!(message.role.as_str(), "user" | "assistant")
            || message.content.trim().is_empty()
            || message.content.chars().count() > 20_000
    }) {
        return Err("聊天消息格式无效或内容过长".into());
    }
    state
        .cancelled_chat_requests
        .lock()
        .unwrap()
        .remove(&request_id);
    let (base_url, model) = {
        let settings = state.settings.lock().unwrap();
        (settings.ai_base_url.clone(), settings.ai_model.clone())
    };
    let api_key = get_ai_api_key()?;
    let mut api_messages = vec![ChatMessage {
        role: "system".into(),
        content: "你是一只友好、自然、简洁的桌面宠物。使用用户的语言回答，不要声称能读取未提供的屏幕、文件或键盘内容。".into(),
    }];
    api_messages.extend(messages);
    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let result: Result<(), String> = async {
        let response = reqwest::Client::new()
            .post(endpoint)
            .bearer_auth(api_key)
            .json(&serde_json::json!({
                "model": model,
                "messages": api_messages,
                "stream": true,
                "thinking": { "type": "disabled" }
            }))
            .send()
            .await
            .map_err(|e| format!("无法连接 AI 服务：{e}"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let detail: String = body.chars().take(500).collect();
            return Err(format!("AI 服务返回 {status}：{detail}"));
        }
        use futures_util::StreamExt;
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        while let Some(chunk) = stream.next().await {
            if state
                .cancelled_chat_requests
                .lock()
                .unwrap()
                .contains(&request_id)
            {
                return Ok(());
            }
            let chunk = chunk.map_err(|e| format!("读取 AI 回复失败：{e}"))?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            buffer = buffer.replace("\r\n", "\n");
            while let Some(end) = buffer.find("\n\n") {
                let event = buffer[..end].to_owned();
                buffer.drain(..end + 2);
                let (deltas, done) = parse_chat_sse_event(&event)?;
                for delta in deltas {
                    let _ = app.emit_to(
                        "chat",
                        "chat-stream-delta",
                        ChatDelta {
                            request_id: request_id.clone(),
                            delta,
                        },
                    );
                }
                if done {
                    return Ok(());
                }
            }
        }
        Ok(())
    }
    .await;
    state
        .cancelled_chat_requests
        .lock()
        .unwrap()
        .remove(&request_id);
    result
}
#[tauri::command]
fn pet_data_dir(app: AppHandle) -> Result<String, String> {
    let p = pets_dir(&app)?;
    fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    Ok(p.to_string_lossy().into_owned())
}
#[tauri::command]
fn delete_custom_pet(app: AppHandle, id: String) -> Result<(), String> {
    let p = pets_dir(&app)?.join(&id);
    if !p.exists() {
        return Err("不能删除内置宠物或不存在的宠物".into());
    };
    fs::remove_dir_all(p).map_err(|e| e.to_string())
}
#[tauri::command]
fn import_pet(app: AppHandle, source_path: String, conflict: String) -> Result<PetRecord, String> {
    let src = PathBuf::from(source_path);
    let mut m = validate_package(&src)?;
    let root = pets_dir(&app)?;
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let mut target = root.join(&m.id);
    if target.exists() {
        match conflict.as_str() {
            "overwrite" => fs::remove_dir_all(&target).map_err(|e| e.to_string())?,
            "rename" => {
                let base = m.id.clone();
                let mut n = 2;
                while target.exists() {
                    m.id = format!("{base}-{n}");
                    target = root.join(&m.id);
                    n += 1
                }
            }
            _ => return Err(format!("ID_CONFLICT:{}；请选择覆盖、重命名或取消", m.id)),
        }
    }
    copy_dir(&src, &target)?;
    if conflict == "rename" {
        fs::write(
            target.join("pet.json"),
            serde_json::to_vec_pretty(&m).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?
    };
    custom_record(target)
}
fn copy_dir(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for e in fs::read_dir(src).map_err(|e| e.to_string())? {
        let e = e.map_err(|e| e.to_string())?;
        let to = dst.join(e.file_name());
        if e.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_dir(&e.path(), &to)?
        } else {
            fs::copy(e.path(), to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html".into()))
        .title("Pet Desk 设置")
        .inner_size(760.0, 640.0)
        .resizable(true)
        .build();
}

fn anchored_panel_position(
    pet_x: i32,
    pet_y: i32,
    pet_width: u32,
    panel_width: u32,
    panel_height: u32,
    monitor_x: i32,
    monitor_y: i32,
    monitor_width: u32,
    monitor_height: u32,
    gap: i32,
) -> (i32, i32) {
    let monitor_right = monitor_x.saturating_add_unsigned(monitor_width);
    let monitor_bottom = monitor_y.saturating_add_unsigned(monitor_height);
    let mut x = pet_x.saturating_add_unsigned(pet_width).saturating_add(gap);
    if x.saturating_add_unsigned(panel_width) > monitor_right {
        x = pet_x
            .saturating_sub_unsigned(panel_width)
            .saturating_sub(gap);
    }
    let max_x = monitor_right.saturating_sub_unsigned(panel_width);
    let max_y = monitor_bottom.saturating_sub_unsigned(panel_height);
    (
        x.clamp(monitor_x, max_x.max(monitor_x)),
        pet_y.clamp(monitor_y, max_y.max(monitor_y)),
    )
}

fn position_panel_beside_pet(app: &AppHandle, label: &str, activate: bool) -> Result<(), String> {
    let pet = app.get_webview_window("pet").ok_or("找不到桌宠窗口")?;
    let panel = app
        .get_webview_window(label)
        .ok_or_else(|| format!("{label} 窗口尚未初始化"))?;
    let pet_position = pet.outer_position().map_err(|e| e.to_string())?;
    let pet_size = pet.outer_size().map_err(|e| e.to_string())?;
    let panel_size = panel.outer_size().map_err(|e| e.to_string())?;
    let monitor = pet
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("找不到桌宠所在的显示器")?;
    let monitor_position = monitor.position();
    let monitor_size = monitor.size();
    let gap = (10.0 * pet.scale_factor().map_err(|e| e.to_string())?).round() as i32;
    let (x, y) = anchored_panel_position(
        pet_position.x,
        pet_position.y,
        pet_size.width,
        panel_size.width,
        panel_size.height,
        monitor_position.x,
        monitor_position.y,
        monitor_size.width,
        monitor_size.height,
        gap,
    );
    panel
        .set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
            x, y,
        )))
        .map_err(|e| e.to_string())?;
    if activate {
        panel.show().map_err(|e| e.to_string())?;
        panel.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn open_action_menu(app: AppHandle) -> Result<(), String> {
    position_panel_beside_pet(&app, "actions", true)
}
#[tauri::command]
fn open_chat(app: AppHandle) -> Result<(), String> {
    position_panel_beside_pet(&app, "chat", true)
}
#[tauri::command]
fn hide_chat(app: AppHandle) -> Result<(), String> {
    app.get_webview_window("chat")
        .ok_or("聊天窗口尚未初始化")?
        .hide()
        .map_err(|e| e.to_string())
}
#[tauri::command]
fn suspend_chat_for_pet_drag(app: AppHandle) -> Result<bool, String> {
    let chat = app.get_webview_window("chat").ok_or("聊天窗口尚未初始化")?;
    let was_visible = chat.is_visible().map_err(|e| e.to_string())?;
    if was_visible {
        chat.hide().map_err(|e| e.to_string())?;
    }
    Ok(was_visible)
}
#[tauri::command]
fn restore_chat_after_pet_drag(app: AppHandle) -> Result<(), String> {
    position_panel_beside_pet(&app, "chat", true)
}
#[tauri::command]
fn toggle_chat(app: AppHandle) -> Result<(), String> {
    let chat = app.get_webview_window("chat").ok_or("聊天窗口尚未初始化")?;
    if chat.is_visible().map_err(|e| e.to_string())? {
        chat.hide().map_err(|e| e.to_string())
    } else {
        position_panel_beside_pet(&app, "chat", true)
    }
}
#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}
fn tray(app: &AppHandle) {
    let show = MenuItem::with_id(app, "show", "显示 / 隐藏", true, None::<&str>).unwrap();
    let chat = MenuItem::with_id(app, "chat", "和宠物聊天", true, None::<&str>).unwrap();
    let actions = MenuItem::with_id(app, "actions", "打开动作面板", true, None::<&str>).unwrap();
    let click = CheckMenuItem::with_id(app, "click", "鼠标穿透", true, true, None::<&str>).unwrap();
    let top = CheckMenuItem::with_id(app, "top", "始终置顶", true, true, None::<&str>).unwrap();
    let drag = CheckMenuItem::with_id(app, "drag", "允许拖动", true, true, None::<&str>).unwrap();
    let auto = CheckMenuItem::with_id(app, "auto", "开机自启", true, true, None::<&str>).unwrap();
    let settings = MenuItem::with_id(app, "settings", "打开设置", true, None::<&str>).unwrap();
    let reset = MenuItem::with_id(app, "reset", "重置位置", true, None::<&str>).unwrap();
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>).unwrap();
    let menu = Menu::with_items(
        app,
        &[
            &chat, &actions, &show, &click, &top, &drag, &auto, &settings, &reset, &quit,
        ],
    )
    .unwrap();
    let handle = app.clone();

    let mut tray = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("Pet Desk")
        .title("Pet Desk");
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }

    tray.on_menu_event(move |_, event| {
        let id = event.id().as_ref();
        match id {
            "chat" => {
                if let Some(window) = handle.get_webview_window("pet") {
                    let _ = window.show();
                }
                let _ = position_panel_beside_pet(&handle, "chat", true);
            }
            "actions" => {
                if let Some(window) = handle.get_webview_window("pet") {
                    let _ = window.show();
                }
                let _ = position_panel_beside_pet(&handle, "actions", true);
            }
            "show" => {
                if let Some(window) = handle.get_webview_window("pet") {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.hide();
                    } else {
                        let _ = window.show();
                    }
                }
            }
            "settings" => open_settings(&handle),
            "reset" => {
                let _ = handle.emit("reset-position", ());
            }
            "quit" => handle.exit(0),
            "click" | "top" | "drag" | "auto" => {
                let state = handle.state::<AppState>();
                let mut settings = state.settings.lock().unwrap().clone();
                match id {
                    "click" => settings.click_through = !settings.click_through,
                    "top" => settings.always_on_top = !settings.always_on_top,
                    "drag" => settings.drag_enabled = !settings.drag_enabled,
                    _ => settings.autostart = !settings.autostart,
                }
                drop(state);
                let _ = save_settings(handle.clone(), handle.state(), settings);
            }
            _ => {}
        }
    })
    .build(app)
    .unwrap();
}
#[cfg(target_os = "macos")]
fn start_global_input_listener(app: AppHandle) {
    std::thread::spawn(move || {
        use core_foundation::runloop::CFRunLoop;
        use core_graphics::event::{
            CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
            CallbackResult,
        };
        let result = CGEventTap::with_enabled(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            vec![CGEventType::KeyDown],
            move |_, _, _| {
                let enabled = app
                    .state::<AppState>()
                    .settings
                    .lock()
                    .map(|settings| settings.input_listening_enabled)
                    .unwrap_or(false);
                if enabled {
                    let _ = app.emit("global-key-activity", ());
                }
                CallbackResult::Keep
            },
            CFRunLoop::run_current,
        );
        if result.is_err() {
            eprintln!("global keyboard listener could not start; allow Input Monitoring access");
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn start_global_input_listener(app: AppHandle) {
    std::thread::spawn(move || {
        let callback = move |event: rdev::Event| {
            // Only report activity. Never inspect, store, or transmit the pressed key.
            if !matches!(event.event_type, rdev::EventType::KeyPress(_)) {
                return;
            }
            let enabled = app
                .state::<AppState>()
                .settings
                .lock()
                .map(|settings| settings.input_listening_enabled)
                .unwrap_or(false);
            if enabled {
                let _ = app.emit("global-key-activity", ());
            }
        };
        if let Err(error) = rdev::listen(callback) {
            eprintln!("global keyboard listener stopped: {error:?}");
        }
    });
}
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let s = read_settings(app.handle());
            app.manage(AppState {
                settings: Mutex::new(s.clone()),
                cancelled_chat_requests: Mutex::new(HashSet::new()),
            });
            for config in app
                .config()
                .app
                .windows
                .iter()
                .filter(|config| matches!(config.label.as_str(), "actions" | "chat"))
            {
                WebviewWindowBuilder::from_config(app.handle(), config)?.build()?;
            }
            if let (Some(x), Some(y)) = (s.x, s.y) {
                if let Some(window) = app.get_webview_window("pet") {
                    let _ = window.set_position(tauri::Position::Physical(
                        tauri::PhysicalPosition::new(x, y),
                    ));
                }
            }
            let _ = fs::create_dir_all(pets_dir(app.handle()).unwrap());
            if s.autostart {
                let _ = app.autolaunch().enable();
            }
            tray(app.handle());
            start_global_input_listener(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_pets,
            pet_spritesheet,
            get_settings,
            get_ai_service_config,
            save_settings,
            save_ai_service_config,
            delete_ai_api_key,
            send_chat_message,
            cancel_chat_request,
            pet_data_dir,
            delete_custom_pet,
            import_pet,
            open_action_menu,
            open_chat,
            hide_chat,
            suspend_chat_for_pet_drag,
            restore_chat_after_pet_drag,
            toggle_chat,
            quit_app
        ])
        .on_window_event(|w, e| match e {
            tauri::WindowEvent::Moved(position) if w.label() == "pet" => {
                let state = w.state::<AppState>();
                let snapshot = {
                    let mut settings = state.settings.lock().unwrap();
                    if settings.x == Some(position.x) && settings.y == Some(position.y) {
                        return;
                    }
                    settings.x = Some(position.x);
                    settings.y = Some(position.y);
                    settings.clone()
                };
                if let Err(error) = write_settings(w.app_handle(), &snapshot) {
                    eprintln!("failed to save pet window position: {error}");
                }
                for label in ["chat", "actions"] {
                    if w.app_handle()
                        .get_webview_window(label)
                        .and_then(|panel| panel.is_visible().ok())
                        .unwrap_or(false)
                    {
                        let _ = position_panel_beside_pet(w.app_handle(), label, false);
                    }
                }
            }
            tauri::WindowEvent::CloseRequested { .. } if w.label() == "pet" => {
                w.app_handle().exit(0);
            }
            tauri::WindowEvent::CloseRequested { api, .. } if w.label() == "chat" => {
                api.prevent_close();
                let _ = w.hide();
            }
            tauri::WindowEvent::Focused(false) if w.label() == "actions" => {
                let _ = w.hide();
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("tauri failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn default_settings_match_product_defaults() {
        let s = Settings::default();
        assert!(s.click_through && s.always_on_top && s.drag_enabled && s.autostart);
        assert_eq!(s.current_pet_id, "ikunchick");
        assert_eq!(s.animation_speed, 1.0);
        assert!(s.input_listening_enabled);
        assert!(s.input_animation_by_pet.is_empty());
        assert_eq!(s.ai_provider, "openai-compatible");
        assert_eq!(s.ai_base_url, "https://api.deepseek.com");
        assert_eq!(s.ai_model, "deepseek-v4-flash");
    }
    #[test]
    fn old_settings_default_animation_speed() {
        let s:Settings=serde_json::from_str(r#"{"currentPetId":"ikunchick","scale":1.5,"clickThrough":false,"alwaysOnTop":true,"dragEnabled":true,"autostart":false,"x":10,"y":20}"#).unwrap();
        assert_eq!(s.animation_speed, 1.0);
        assert!(s.input_listening_enabled);
        assert!(s.input_animation_by_pet.is_empty());
        assert_eq!(s.ai_provider, "openai-compatible");
        assert_eq!(s.ai_base_url, "https://api.deepseek.com");
        assert_eq!(s.ai_model, "deepseek-v4-flash");
        assert_eq!(s.scale, 1.5);
        assert!(!s.click_through);
        assert_eq!(s.x, Some(10));
        assert_eq!(s.y, Some(20));
    }
    #[test]
    fn input_animation_selection_persists_per_pet() {
        let mut settings = Settings::default();
        settings
            .input_animation_by_pet
            .insert("ikunchick".into(), "waving".into());
        settings
            .input_animation_by_pet
            .insert("another-pet".into(), "review".into());
        let restored: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(restored.input_animation_by_pet["ikunchick"], "waving");
        assert_eq!(restored.input_animation_by_pet["another-pet"], "review");
    }
    #[test]
    fn panel_position_prefers_right_then_flips_left_at_screen_edge() {
        assert_eq!(
            anchored_panel_position(100, 100, 192, 380, 520, 0, 0, 1440, 900, 10),
            (302, 100)
        );
        assert_eq!(
            anchored_panel_position(1200, 500, 192, 380, 520, 0, 0, 1440, 900, 10),
            (810, 380)
        );
    }
    #[test]
    fn validates_ai_service_configuration() {
        let valid = AiServiceConfigInput {
            provider: "openai-compatible".into(),
            base_url: "https://api.example.com/v1".into(),
            model: "example-model".into(),
        };
        assert!(validate_ai_service_config(&valid).is_ok());
        let local = AiServiceConfigInput {
            base_url: "http://localhost:11434/v1".into(),
            ..valid
        };
        assert!(validate_ai_service_config(&local).is_ok());
        let insecure_remote = AiServiceConfigInput {
            base_url: "http://api.example.com/v1".into(),
            ..local
        };
        assert!(validate_ai_service_config(&insecure_remote)
            .unwrap_err()
            .contains("HTTPS"));
        let missing_model = AiServiceConfigInput {
            model: " ".into(),
            ..insecure_remote
        };
        assert!(validate_ai_service_config(&missing_model)
            .unwrap_err()
            .contains("模型"));
    }
    #[test]
    fn parses_streaming_chat_events_and_keep_alive_comments() {
        let event = concat!(
            ": keep-alive\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"你\"}}]}\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"好\"}}]}\n"
        );
        let (deltas, done) = parse_chat_sse_event(event).unwrap();
        assert_eq!(deltas, ["你", "好"]);
        assert!(!done);

        let (deltas, done) = parse_chat_sse_event("data: [DONE]\n").unwrap();
        assert!(deltas.is_empty());
        assert!(done);
    }
    #[test]
    fn rejects_bad_manifest() {
        let d = std::env::temp_dir().join(format!("pet-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&d).unwrap();
        fs::File::create(d.join("pet.json")).unwrap().write_all(br#"{"id":"bad id","displayName":"Bad","description":"","spriteVersionNumber":2,"spritesheetPath":"spritesheet.webp","kind":"animal"}"#).unwrap();
        assert!(validate_package(&d).unwrap_err().contains("id"));
        let _ = fs::remove_dir_all(d);
    }
    #[test]
    fn missing_sprite_version_defaults_to_v1() {
        let manifest:PetManifest=serde_json::from_str(r#"{"id":"v1-pet","displayName":"V1","description":"","spritesheetPath":"spritesheet.webp","kind":"animal"}"#).unwrap();
        assert_eq!(manifest.sprite_version_number, 1);
    }
    #[test]
    fn validates_sheet_size_and_alpha() {
        assert!(validate_sheet_properties((W, H_V1), true, 1).is_ok());
        assert!(validate_sheet_properties((W, H_V2), true, 2).is_ok());
        assert!(validate_sheet_properties((W, H_V2), true, 1)
            .unwrap_err()
            .contains("v1"));
        assert!(validate_sheet_properties((1, H_V2), true, 2)
            .unwrap_err()
            .contains("尺寸"));
        assert!(validate_sheet_properties((W, H_V2), false, 2)
            .unwrap_err()
            .contains("透明"));
    }
}
