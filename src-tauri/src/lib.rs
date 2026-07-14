use image::GenericImageView;
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}, sync::Mutex};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Emitter};
use tauri::menu::{Menu, MenuItem, CheckMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri_plugin_autostart::ManagerExt;

const W: u32 = 1536; const H: u32 = 2288;
const BUILTIN_MANIFEST: &str = include_str!("../resources/builtin/pet.json");
const BUILTIN_SPRITESHEET: &[u8] = include_bytes!("../resources/builtin/spritesheet.webp");
#[derive(Clone, Serialize, Deserialize, Debug)] #[serde(rename_all = "camelCase")]
pub struct PetManifest { pub id: String, pub display_name: String, pub description: String, pub sprite_version_number: u32, pub spritesheet_path: String, pub kind: String }
#[derive(Clone, Serialize)] pub struct PetRecord { manifest: PetManifest, source: String, path: String }
#[derive(Clone, Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
pub struct Settings { current_pet_id: String, scale: f64, click_through: bool, always_on_top: bool, drag_enabled: bool, autostart: bool, x: Option<i32>, y: Option<i32> }
impl Default for Settings { fn default() -> Self { Self { current_pet_id:"ikunchick".into(), scale:1.0, click_through:true, always_on_top:true, drag_enabled:true, autostart:true, x:None, y:None } } }
pub struct AppState { settings: Mutex<Settings> }

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> { app.path().app_data_dir().map_err(|e| e.to_string()) }
fn pets_dir(app: &AppHandle) -> Result<PathBuf, String> { Ok(data_dir(app)?.join("pets")) }
fn config_path(app: &AppHandle) -> Result<PathBuf, String> { Ok(data_dir(app)?.join("settings.json")) }
fn write_settings(app: &AppHandle, s: &Settings) -> Result<(), String> { let p=config_path(app)?; fs::create_dir_all(p.parent().unwrap()).map_err(|e|e.to_string())?; fs::write(p, serde_json::to_vec_pretty(s).map_err(|e|e.to_string())?).map_err(|e|e.to_string()) }
fn read_settings(app: &AppHandle) -> Settings { config_path(app).ok().and_then(|p| fs::read(p).ok()).and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_default() }
fn builtin_manifest() -> Result<PetManifest, String> {
  serde_json::from_str(BUILTIN_MANIFEST).map_err(|e|format!("内置宠物 manifest 无效：{e}"))
}

pub fn validate_package(dir: &Path) -> Result<PetManifest, String> {
  let manifest_path=dir.join("pet.json"); let bytes=fs::read(&manifest_path).map_err(|_|format!("找不到 pet.json：{}", manifest_path.display()))?;
  let m:PetManifest=serde_json::from_slice(&bytes).map_err(|e|format!("pet.json 不是合法 JSON：{e}"))?;
  if m.id.trim().is_empty() || !m.id.chars().all(|c|c.is_ascii_alphanumeric() || c=='-' || c=='_') { return Err("pet.json 的 id 必须为非空 ASCII 字母、数字、- 或 _".into()) }
  if m.display_name.trim().is_empty() || m.kind.trim().is_empty() { return Err("pet.json 必须包含 displayName 和 kind".into()) }
  if m.sprite_version_number != 2 { return Err(format!("当前仅支持 spriteVersionNumber: 2，收到 {}",m.sprite_version_number)) }
  let sheet=dir.join(&m.spritesheet_path); if sheet.extension().and_then(|x|x.to_str()).map(|x|x.eq_ignore_ascii_case("webp")) != Some(true) { return Err("v1 仅支持 WebP spritesheet".into()) }
  let reader=image::ImageReader::open(&sheet).map_err(|_|format!("找不到精灵图：{}",sheet.display()))?.with_guessed_format().map_err(|e|format!("不能识别精灵图格式：{e}"))?;
  if reader.format() != Some(image::ImageFormat::WebP) { return Err("精灵图内容必须是 WebP 格式".into()) }
  let img=reader.decode().map_err(|e|format!("无法解码 WebP 精灵图：{e}"))?;
  validate_sheet_properties(img.dimensions(), img.color().has_alpha())?;
  Ok(m)
}
fn validate_sheet_properties(dimensions:(u32,u32), has_alpha:bool)->Result<(),String>{if dimensions != (W,H){return Err(format!("精灵图尺寸必须为 {W}×{H}，实际为 {}×{}",dimensions.0,dimensions.1))}if !has_alpha{return Err("精灵图必须包含透明通道".into())}Ok(())}
fn custom_record(dir:PathBuf) -> Result<PetRecord,String> { let m=validate_package(&dir)?; Ok(PetRecord{manifest:m,source:"custom".into(),path:dir.to_string_lossy().into_owned()}) }
#[tauri::command] fn list_pets(app:AppHandle)->Result<Vec<PetRecord>,String>{
  let mut pets=vec![PetRecord{manifest:builtin_manifest()?,source:"builtin".into(),path:"builtin://ikunchick".into()}];
  let dir=pets_dir(&app)?; if let Ok(entries)=fs::read_dir(dir) { for e in entries.flatten() { if e.path().is_dir() { if let Ok(p)=custom_record(e.path()){pets.push(p)} } } }; Ok(pets)
}
#[tauri::command] fn pet_spritesheet(app:AppHandle,id:String)->Result<tauri::ipc::Response,String>{
  if id == "ikunchick" { return Ok(tauri::ipc::Response::new(BUILTIN_SPRITESHEET.to_vec())) }
  if id.trim().is_empty() || !id.chars().all(|c|c.is_ascii_alphanumeric() || c=='-' || c=='_') { return Err("宠物 id 无效".into()) }
  let dir=pets_dir(&app)?.join(&id); let manifest=validate_package(&dir)?;
  if manifest.id != id { return Err("宠物目录与 manifest id 不一致".into()) }
  let bytes=fs::read(dir.join(manifest.spritesheet_path)).map_err(|e|format!("读取宠物图集失败：{e}"))?;
  Ok(tauri::ipc::Response::new(bytes))
}
#[tauri::command] fn get_settings(state:tauri::State<AppState>)->Settings{state.settings.lock().unwrap().clone()}
#[tauri::command] fn save_settings(app:AppHandle,state:tauri::State<AppState>,settings:Settings)->Result<(),String>{
  if let Some(w)=app.get_webview_window("pet") { w.set_always_on_top(settings.always_on_top).map_err(|e|e.to_string())?; w.set_ignore_cursor_events(settings.click_through).map_err(|e|e.to_string())?; }
  if settings.autostart { let _=app.autolaunch().enable(); } else { let _=app.autolaunch().disable(); }
  write_settings(&app,&settings)?;
  *state.settings.lock().unwrap()=settings.clone();
  let _=app.emit("settings-changed", &settings);
  Ok(())
}
#[tauri::command] fn pet_data_dir(app:AppHandle)->Result<String,String>{let p=pets_dir(&app)?;fs::create_dir_all(&p).map_err(|e|e.to_string())?;Ok(p.to_string_lossy().into_owned())}
#[tauri::command] fn delete_custom_pet(app:AppHandle,id:String)->Result<(),String>{ let p=pets_dir(&app)?.join(&id); if !p.exists(){return Err("不能删除内置宠物或不存在的宠物".into())}; fs::remove_dir_all(p).map_err(|e|e.to_string()) }
#[tauri::command] fn import_pet(app:AppHandle,source_path:String,conflict:String)->Result<PetRecord,String>{
  let src=PathBuf::from(source_path); let mut m=validate_package(&src)?; let root=pets_dir(&app)?;fs::create_dir_all(&root).map_err(|e|e.to_string())?;let mut target=root.join(&m.id);
  if target.exists(){ match conflict.as_str(){"overwrite"=>fs::remove_dir_all(&target).map_err(|e|e.to_string())?,"rename"=>{let base=m.id.clone();let mut n=2;while target.exists(){m.id=format!("{base}-{n}");target=root.join(&m.id);n+=1}},_=>return Err(format!("ID_CONFLICT:{}；请选择覆盖、重命名或取消",m.id))} }
  copy_dir(&src,&target)?; if conflict=="rename" { fs::write(target.join("pet.json"),serde_json::to_vec_pretty(&m).map_err(|e|e.to_string())?).map_err(|e|e.to_string())? }; custom_record(target)
}
fn copy_dir(src:&Path,dst:&Path)->Result<(),String>{fs::create_dir_all(dst).map_err(|e|e.to_string())?;for e in fs::read_dir(src).map_err(|e|e.to_string())?{let e=e.map_err(|e|e.to_string())?;let to=dst.join(e.file_name());if e.file_type().map_err(|e|e.to_string())?.is_dir(){copy_dir(&e.path(),&to)?}else{fs::copy(e.path(),to).map_err(|e|e.to_string())?;}}Ok(())}

fn open_settings(app:&AppHandle){if let Some(w)=app.get_webview_window("settings"){let _=w.show();let _=w.set_focus();return}let _=WebviewWindowBuilder::new(app,"settings",WebviewUrl::App("index.html?settings=1".into())).title("Pet Desk 设置").inner_size(760.0,640.0).resizable(true).build();}
fn tray(app: &AppHandle) {
    let show = MenuItem::with_id(app, "show", "显示 / 隐藏", true, None::<&str>).unwrap();
    let click = CheckMenuItem::with_id(app, "click", "鼠标穿透", true, true, None::<&str>).unwrap();
    let top = CheckMenuItem::with_id(app, "top", "始终置顶", true, true, None::<&str>).unwrap();
    let drag = CheckMenuItem::with_id(app, "drag", "允许拖动", true, true, None::<&str>).unwrap();
    let auto = CheckMenuItem::with_id(app, "auto", "开机自启", true, true, None::<&str>).unwrap();
    let settings = MenuItem::with_id(app, "settings", "打开设置", true, None::<&str>).unwrap();
    let reset = MenuItem::with_id(app, "reset", "重置位置", true, None::<&str>).unwrap();
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>).unwrap();
    let menu = Menu::with_items(app, &[&show, &click, &top, &drag, &auto, &settings, &reset, &quit]).unwrap();
    let handle = app.clone();

    TrayIconBuilder::new()
        .menu(&menu)
        .on_menu_event(move |_, event| {
            let id = event.id().as_ref();
            match id {
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
pub fn run(){tauri::Builder::default().plugin(tauri_plugin_dialog::init()).plugin(tauri_plugin_opener::init()).plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent,None)).setup(|app|{let s=read_settings(app.handle());app.manage(AppState{settings:Mutex::new(s.clone())});let _=fs::create_dir_all(pets_dir(app.handle()).unwrap());if s.autostart{let _=app.autolaunch().enable();}tray(app.handle());Ok(())}).invoke_handler(tauri::generate_handler![list_pets,pet_spritesheet,get_settings,save_settings,pet_data_dir,delete_custom_pet,import_pet]).on_window_event(|w,e|if let tauri::WindowEvent::CloseRequested{api,..}=e{if w.label()=="pet"{api.prevent_close();let _=w.hide();}}).run(tauri::generate_context!()).expect("tauri failed")}

#[cfg(test)] mod tests { use super::*; use std::io::Write;
 #[test] fn default_settings_match_product_defaults(){let s=Settings::default();assert!(s.click_through && s.always_on_top && s.drag_enabled && s.autostart);assert_eq!(s.current_pet_id,"ikunchick");}
 #[test] fn rejects_bad_manifest(){let d=std::env::temp_dir().join(format!("pet-test-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&d).unwrap();fs::File::create(d.join("pet.json")).unwrap().write_all(br#"{"id":"bad id"}"#).unwrap();assert!(validate_package(&d).unwrap_err().contains("id"));let _=fs::remove_dir_all(d);}
 #[test] fn validates_sheet_size_and_alpha(){assert!(validate_sheet_properties((W,H),true).is_ok());assert!(validate_sheet_properties((1,H),true).unwrap_err().contains("尺寸"));assert!(validate_sheet_properties((W,H),false).unwrap_err().contains("透明"));}
}
