// 系统托盘：把进程生命周期从窗口上解绑
//
// Engine、Scheduler、飞书长连接本来就跑在 Rust 侧的后台任务里，之前唯一的问题是
// Tauri 在最后一个窗口销毁后就退出进程 —— 关窗口等于杀掉调度器和飞书。
// 这里做三件事：拦下关窗改成隐藏、用托盘图标承载「还活着」的可见性与控制入口、
// 保证退出时先停渠道再真正退出。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Window, Wry};
use tauri_plugin_notification::NotificationExt;

use crate::channel::{ChannelKind, ChannelManager, ChannelStatus};
use crate::config::ConfigStore;
use crate::engine::Engine;

/// 托盘图标 id，运行时靠它取回句柄改 tooltip
const TRAY_ID: &str = "main";
/// 渠道状态轮询间隔。状态几分钟才变一次，5 秒足够跟上；
/// 且只有文案真的变了才会调系统 API，绝大多数 tick 会被整轮跳过
const TICK: Duration = Duration::from_secs(5);

/// 正在退出。必须有 —— app.exit(0) 会去关窗口，而关窗口会触发 CloseRequested，
/// 那里无条件 prevent_close 的话就把自己反锁了，点「退出」毫无反应。
static QUITTING: AtomicBool = AtomicBool::new(false);

/// 「已隐藏到托盘」只提示一次。每次关窗都弹通知太吵，
/// 而用户只需要知道一次「关窗不等于退出」
static HINT_ONCE: Once = Once::new();

/// 需要跨 tick 改文字的菜单项。
///
/// 之所以持有句柄而不是每 tick 用 set_menu 重建整个菜单：重建会在用户正好
/// 打开着菜单时闪烁，set_text 是原地改；顺带也省掉每 5 秒构造一遍菜单树。
struct TrayItems {
    feishu_status: MenuItem<Wry>,
    feishu_toggle: MenuItem<Wry>,
    qq_status: MenuItem<Wry>,
    qq_toggle: MenuItem<Wry>,
    /// 上次渲染的状态文案，用于跳过无变化的 tick
    last: Mutex<String>,
}

pub fn is_quitting() -> bool {
    QUITTING.load(Ordering::SeqCst)
}

/// 建托盘图标与菜单，并启动状态轮询
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    // enabled = false：这些行是只读展示，点它们不该有任何反应
    let feishu_status =
        MenuItem::with_id(app, "feishu_status", "飞书：查询中…", false, None::<&str>)?;
    let feishu_toggle = MenuItem::with_id(app, "toggle_feishu", "启动飞书", true, None::<&str>)?;
    let qq_status = MenuItem::with_id(app, "qq_status", "QQ：查询中…", false, None::<&str>)?;
    let qq_toggle = MenuItem::with_id(app, "toggle_qq", "启动 QQ", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &show,
            &PredefinedMenuItem::separator(app)?,
            &feishu_status,
            &feishu_toggle,
            &qq_status,
            &qq_toggle,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Avalon")
        // Windows 习惯是左键单击唤起主界面、右键出菜单，这里把左键让给窗口
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, event.id.as_ref()))
        .on_tray_icon_event(|tray, event| {
            // 左键单击 = 显示窗口，不做 toggle：窗口可能可见但被别的窗口盖住，
            // 此时 toggle 会把它藏起来，与用户「我要看它」的意图正好相反
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;

    app.manage(TrayItems {
        feishu_status,
        feishu_toggle,
        qq_status,
        qq_toggle,
        last: Mutex::new(String::new()),
    });

    // 先同步一次，别让菜单停在「查询中…」等满 5 秒
    refresh(app);
    spawn_status_tick(app.clone());
    Ok(())
}

/// 显示并聚焦主窗口。窗口可能处于隐藏、最小化或被遮挡三种状态，三步都得走
pub fn show_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

/// 隐藏窗口到托盘。
///
/// 用 hide 而非 destroy：WebView 留在内存里，重开窗口瞬间可用，会话状态和
/// task-finished 订阅都还在。代价是几十 MB 常驻，对随时唤起的助手划算。
pub fn hide_to_tray(window: &Window) {
    let _ = window.hide();

    HINT_ONCE.call_once(|| {
        let result = window
            .app_handle()
            .notification()
            .builder()
            .title("Avalon 仍在后台运行")
            .body("外部渠道与定时任务照常工作。托盘图标可重新打开窗口或退出。")
            .show();
        if let Err(e) = result {
            log::warn!(target: "tray", "后台提示通知发送失败: {e}");
        }
    });
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "show" => show_window(app),
        "toggle_feishu" => {
            toggle_channel(app, ChannelKind::Feishu);
            // 立即刷新，不等下一个 tick —— 点完菜单要马上看到文字变化
            refresh(app);
        }
        "toggle_qq" => {
            toggle_channel(app, ChannelKind::Qq);
            refresh(app);
        }
        "quit" => quit(app),
        _ => {}
    }
}

/// 启停渠道。与 commands.rs 的 feishu_start / qq_start 走同一条路径
fn toggle_channel(app: &AppHandle, kind: ChannelKind) {
    let channels = app.state::<Arc<ChannelManager>>();
    if channels.is_running(kind) {
        channels.stop(kind);
        log::info!(target: "tray", "{} 渠道已停止", kind.as_str());
        return;
    }

    let config = app.state::<ConfigStore>();
    let engine = app.state::<Arc<Engine>>();
    let cfg = config.get();
    let result = match kind {
        ChannelKind::Feishu => channels.start_feishu(cfg.feishu, engine.inner().clone()),
        ChannelKind::Qq => channels.start_qq(cfg.qq, engine.inner().clone()),
    };
    match result {
        Ok(()) => log::info!(target: "tray", "{} 渠道已启动", kind.as_str()),
        Err(e) => log::warn!(target: "tray", "{} 渠道启动失败: {e}", kind.as_str()),
    }
}

/// 退出应用。顺序不能反：置位 → 停渠道 → 退出
fn quit(app: &AppHandle) {
    QUITTING.store(true, Ordering::SeqCst);

    // 不停渠道直接退的话，长连接不发关闭帧，飞书侧要等超时才释放连接配额（上限 50），
    // 反复重启会把配额耗光，表现为握手返回 handshake-autherrcode = 1000040350
    app.state::<Arc<ChannelManager>>().stop_all();

    app.exit(0);
}

/// 后台轮询渠道状态，同步到菜单文字与 tooltip
fn spawn_status_tick(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(TICK);
        // 首个 tick 立即触发，而 setup 里已经 refresh 过一次，跳掉它
        ticker.tick().await;
        loop {
            ticker.tick().await;
            if is_quitting() {
                return;
            }
            refresh(&app);
        }
    });
}

/// 状态 → 菜单文字 + tooltip。文案没变就整轮跳过，不碰任何系统 API
fn refresh(app: &AppHandle) {
    let Some(items) = app.try_state::<TrayItems>() else {
        return;
    };
    let channels = app.state::<Arc<ChannelManager>>();
    let feishu = channels.status(ChannelKind::Feishu);
    let qq = channels.status(ChannelKind::Qq);

    let feishu_text = describe(&feishu);
    let qq_text = describe(&qq);
    let combined = format!("{feishu_text}|{qq_text}");

    {
        // 锁只用来比对与记录，set_text 那些系统调用放到锁外面
        let mut last = items.last.lock().unwrap_or_else(|e| e.into_inner());
        if *last == combined {
            return;
        }
        last.clone_from(&combined);
    }

    let _ = items.feishu_status.set_text(format!("飞书：{feishu_text}"));
    let _ = items.qq_status.set_text(format!("QQ：{qq_text}"));
    let _ = items.feishu_toggle.set_text(if feishu.is_active() {
        "停止飞书"
    } else {
        "启动飞书"
    });
    let _ = items.qq_toggle.set_text(if qq.is_active() {
        "停止 QQ"
    } else {
        "启动 QQ"
    });

    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        // 错误原因只进 tooltip：菜单项塞不下，而 tooltip 本来就是给细节用的
        let mut tip = format!("Avalon — 飞书：{feishu_text}；QQ：{qq_text}");
        if let ChannelStatus::Error { message } = &feishu {
            tip.push_str(&format!("\n飞书错误：{message}"));
        }
        if let ChannelStatus::Error { message } = &qq {
            tip.push_str(&format!("\nQQ 错误：{message}"));
        }
        let _ = tray.set_tooltip(Some(tip));
    }
}

fn describe(status: &ChannelStatus) -> String {
    match status {
        ChannelStatus::Disabled => "未启用",
        ChannelStatus::Stopped => "已停止",
        ChannelStatus::Connecting => "连接中",
        ChannelStatus::Running => "运行中",
        ChannelStatus::Reconnecting => "重连中",
        ChannelStatus::Error { .. } => "连接错误",
    }
    .to_string()
}
