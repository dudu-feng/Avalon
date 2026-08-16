// 基础文件/终端工具实现
//
// 5 个工具：read_file / write_file / delete_file / get_directory_contents 同步（std），
// run_shell_command 异步（tokio::process，带超时 + 进程树清理）。
// 工具签名统一：(args: &Value) -> String，参数错误/执行错误均以字符串返回。

#![allow(dead_code)] // tool 模块供未来 engine 引用，当前无调用方，接入后移除

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use futures_util::future;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};

/// 读文件上限：5MB，超限截断（防大文件塞爆上下文）
const MAX_READ: usize = 5 * 1024 * 1024;
/// 终端命令超时
const SHELL_TIMEOUT: Duration = Duration::from_secs(30);
/// 命令输出上限：stdout/stderr 各 64KB
const OUT_LIMIT: usize = 64 * 1024;

/// 读取指定文件内容（UTF-8 文本），超 5MB 截断
pub fn read_file(args: &Value) -> String {
    let Some(file_path) = args.get("file_path").and_then(Value::as_str) else {
        return "参数错误: 缺少 file_path 或类型应为字符串".to_string();
    };

    let file = match std::fs::File::open(file_path) {
        Ok(f) => f,
        Err(e) => return format!("读取文件失败: {e}"),
    };

    // 多读 1 字节判断是否超限
    let mut buf = Vec::new();
    if let Err(e) = file.take((MAX_READ + 1) as u64).read_to_end(&mut buf) {
        return format!("读取文件失败: {e}");
    }
    let truncated = buf.len() > MAX_READ;
    if truncated {
        buf.truncate(MAX_READ);
    }

    match String::from_utf8(buf) {
        Ok(content) => {
            if truncated {
                format!("{content}\n...[文件过大，已截断前 5MB]")
            } else {
                content
            }
        }
        Err(_) => "读取文件失败: 文件不是有效的 UTF-8 文本（可能是二进制文件）".to_string(),
    }
}

/// 创建或覆盖写入文件，自动创建父目录
pub fn write_file(args: &Value) -> String {
    let Some(file_path) = args.get("file_path").and_then(Value::as_str) else {
        return "参数错误: 缺少 file_path 或类型应为字符串".to_string();
    };
    let Some(content) = args.get("content").and_then(Value::as_str) else {
        return "参数错误: 缺少 content 或类型应为字符串".to_string();
    };

    // 自动建父目录（省去一次 mkdir 工具调用）
    if let Some(parent) = Path::new(file_path).parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return format!("写入文件失败: {e}");
            }
        }
    }

    match std::fs::write(file_path, content) {
        Ok(_) => format!("文件 {file_path} 写入成功"),
        Err(e) => format!("写入文件失败: {e}"),
    }
}

/// 删除指定文件（仅文件，删目录报错）
pub fn delete_file(args: &Value) -> String {
    let Some(file_path) = args.get("file_path").and_then(Value::as_str) else {
        return "参数错误: 缺少 file_path 或类型应为字符串".to_string();
    };

    match std::fs::remove_file(file_path) {
        Ok(_) => format!("文件 {file_path} 已删除"),
        Err(e) => format!("删除文件失败: {e}"),
    }
}

/// 获取目录下文件与子目录，目录在前文件在后、各自按名排序
pub fn get_directory_contents(args: &Value) -> String {
    let Some(dir_path) = args.get("directory_path").and_then(Value::as_str) else {
        return "参数错误: 缺少 directory_path 或类型应为字符串".to_string();
    };

    let entries = match std::fs::read_dir(dir_path) {
        Ok(e) => e,
        Err(e) => return format!("获取目录内容失败: {e}"),
    };

    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        match entry.file_type() {
            Ok(ft) if ft.is_dir() => dirs.push(name),
            _ => files.push(name),
        }
    }
    dirs.sort();
    files.sort();

    let mut out = String::new();
    for d in &dirs {
        out.push_str(&format!("[D] {d}\n"));
    }
    for f in &files {
        out.push_str(&format!("[F] {f}\n"));
    }

    if out.is_empty() {
        "目录为空".to_string()
    } else {
        out.trim_end().to_string()
    }
}

/// 在终端执行命令，返回 stdout+stderr，超时 30s 终止
pub async fn run_shell_command(args: &Value) -> String {
    let Some(command) = args.get("command").and_then(Value::as_str) else {
        return "参数错误: 缺少 command 或类型应为字符串".to_string();
    };

    #[cfg(target_os = "windows")]
    let (shell, flag) = ("cmd", "/C");
    #[cfg(not(target_os = "windows"))]
    let (shell, flag) = ("sh", "-c");

    let mut child = match Command::new(shell)
        .arg(flag)
        .arg(command)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return format!("执行命令失败: {e}"),
    };
    let pid = child.id();

    match tokio::time::timeout(SHELL_TIMEOUT, collect_output(&mut child)).await {
        Ok(Ok((stdout, stderr, status))) => {
            let code = status.code().unwrap_or(-1);
            let mut out = decode_output(&stdout);
            let err = decode_output(&stderr);
            if !out.is_empty() && !err.is_empty() {
                out.push('\n');
            }
            out.push_str(&err);
            if code != 0 {
                out.push_str(&format!("\n[退出码: {code}]"));
            }
            out
        }
        Ok(Err(e)) => format!("执行命令失败: {e}"),
        Err(_) => {
            kill_tree(pid);
            format!("执行命令超时（{} 秒），已终止", SHELL_TIMEOUT.as_secs())
        }
    }
}

/// 并发读 stdout/stderr（各限 OUT_LIMIT）并等待进程退出
async fn collect_output(
    child: &mut Child,
) -> std::io::Result<(Vec<u8>, Vec<u8>, std::process::ExitStatus)> {
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("stdout 未捕获"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("stderr 未捕获"))?;

    let (out, err, status) = future::join3(
        read_limited(stdout, OUT_LIMIT),
        read_limited(stderr, OUT_LIMIT),
        child.wait(),
    )
    .await;

    Ok((out?, err?, status?))
}

/// 读最多 limit 字节
async fn read_limited<R: AsyncRead + Unpin>(mut reader: R, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(limit.min(8192));
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await? {
            0 => break,
            n => {
                if buf.len() + n > limit {
                    buf.extend_from_slice(&chunk[..limit - buf.len()]);
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
        }
    }
    Ok(buf)
}

/// 超时后杀进程树（Windows taskkill /T /F，Unix kill）
fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status();
    }
}

/// 命令输出解码：优先 UTF-8，Windows 上回退 GBK（cmd 默认代码页）
fn decode_output(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    #[cfg(target_os = "windows")]
    {
        let (cow, _, _) = encoding_rs::GBK.decode(bytes);
        return cow.into_owned();
    }
    #[cfg(not(target_os = "windows"))]
    {
        String::from_utf8_lossy(bytes).into_owned()
    }
}
