// md_server_rs - Markdown 文件浏览器 (Rust)
// Usage: md_server_rs [port] [root_dir] [base_path]
//   port:      server port (default: 8899)
//   root_dir:  directory to serve (default: ./docs)
//   base_path: URL prefix for links, e.g. /docs (default: "")
//   支持相对路径和绝对路径
// Fixes:
//   - 左侧目录按字母排序 (locale-aware)
//   - 中文目录跳转正确
//   - 统一 UTF-8 编码
//   - LaTeX 数学公式渲染 (KaTeX)
use pulldown_cmark::{html, Options, Parser};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;

const DEFAULT_PORT: u16 = 8899;

const SUPPORTED_EXTS: &[&str] = &["md", "rs", "py", "toml", "lock", "json", "yaml", "yml", "txt", "ini", "cfg"];

fn is_supported(p: &Path) -> bool {
    p.extension().and_then(|x| x.to_str()).map(|x| SUPPORTED_EXTS.contains(&x)).unwrap_or(false)
}

const HTML_TPL: &str = r##"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} &mdash; MD Viewer</title>
<link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/styles/atom-one-dark.min.css">
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.css">
<style>
* {margin:0;padding:0;box-sizing:border-box;}
body {font:15px/1.7 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;color:#2c3e50;background:#fafafa;display:flex;width:100%;height:100vh;overflow:hidden;}
#sidebar {width:{sidebar_w};min-width:100px;max-width:600px;background:#1a1a2e;color:#eee;padding:20px 16px;overflow-y:auto;flex-shrink:0;position:relative;}
#sidebar a {display:block;color:#a8b2d1;text-decoration:none;padding:3px 8px;border-radius:4px;font-size:13px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
#sidebar a:hover,#sidebar a.active {background:#e94560;color:#fff;}
#sidebar h3 {font-size:14px;color:#e94560;margin:16px 0 8px;text-transform:uppercase;letter-spacing:1px;}
#sidebar .parent-btn {display:block;color:#e94560;font-weight:bold;font-size:14px;padding:8px;margin-bottom:12px;border-bottom:1px solid #333;}
#resizer {width:4px;cursor:col-resize;background:#333;flex-shrink:0;position:relative;}
#resizer:hover,#resizer.dragging {background:#e94560;}
#main {flex:1;overflow-y:auto;padding:32px 48px;background:#fff;}
#main h1 {color:#1a1a2e;border-bottom:3px solid #e94560;padding-bottom:10px;margin:0 0 25px;}
#main h2 {color:#1a1a2e;border-bottom:1px solid #eee;padding-bottom:8px;margin:30px 0 15px;}
#main h3 {color:#1a1a2e;margin:25px 0 10px;}
#main h4 {color:#555;margin:20px 0 8px;}
#main p {margin:12px 0;}
#main a {color:#e94560;}
#main code {color:#e94560;background:#f5f5f5;padding:2px 5px;border-radius:3px;font-size:0.9em;}
#main pre {background:#1a1a2e;color:#abb2bf;padding:16px 20px;border-radius:8px;overflow-x:auto;margin:15px 0;}
#main pre code {color:#abb2bf;background:transparent;padding:0;}
#main table {border-collapse:collapse;width:100%;margin:15px 0;}
#main th,#main td {border:1px solid #ddd;padding:8px 12px;text-align:left;}
#main th {background:#1a1a2e;color:#fff;font-weight:bold;}
#main tr:nth-child(even) {background:#f5f5f5;}
#main blockquote {border-left:4px solid #e94560;margin:15px 0;padding:10px 20px;background:#f9f9f9;border-radius:0 4px 4px 0;}
#main img {max-width:100%;border-radius:6px;margin:10px 0;}
#main hr {border:none;border-top:1px solid #ddd;margin:25px 0;}
#main ul,#main ol {margin:10px 0;padding-left:25px;}
#main li {margin:4px 0;}
#main .hljs {background:transparent;}
.katex-formula {overflow-x:auto;overflow-y:hidden;padding:8px 0;text-align:center;}
</style>
</head><body>
<nav id="sidebar">{nav}</nav>
<div id="resizer"></div>
<main id="main">{content}</main>
<script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/highlight.min.js"></script>
<script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.js"></script>
<script defer src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/contrib/auto-render.min.js" onload="renderMathInElement(document.getElementById('main'),{delimiters:[{left:'$$',right:'$$',display:true},{left:'$',right:'$',display:false}]});"></script>
<script>
(function(){var sidebar=document.getElementById("sidebar");var resizer=document.getElementById("resizer");var isDragging=false;var saved=localStorage.getItem("md_sidebar_w");if(saved)sidebar.style.width=saved+"px";resizer.addEventListener("mousedown",function(e){isDragging=true;e.preventDefault()});document.addEventListener("mousemove",function(e){if(!isDragging)return;var w=Math.max(100,Math.min(600,e.clientX-sidebar.getBoundingClientRect().left));sidebar.style.width=w+"px"});document.addEventListener("mouseup",function(){if(isDragging){isDragging=false;localStorage.setItem("md_sidebar_w",parseInt(sidebar.style.width))}});document.querySelectorAll("#sidebar a").forEach(function(a){if(a.href===location.href||a.href===location.href.split("?")[0])a.classList.add("active")});hljs.highlightAll();})();
</script>
</body></html>"##;

fn main() {
    let args: Vec<String> = env::args().collect();

    // parse port (first optional arg)
    let port: u16 = if args.len() > 1 {
        args[1].parse().unwrap_or(DEFAULT_PORT)
    } else {
        DEFAULT_PORT
    };

    // parse root dir (second optional arg)
    let root_dir: PathBuf = if args.len() > 2 {
        PathBuf::from(&args[2])
    } else {
        let cwd = env::current_dir().expect("Cannot get CWD");
        cwd.join("docs")
    };

    // parse base path (third optional arg, e.g. /docs)
    let base_path: String = if args.len() > 3 {
        let bp = args[3].trim_start_matches('/').to_string();
        if bp.is_empty() { String::new() } else { format!("/{}", bp) }
    } else {
        String::new()
    };

    let served = if root_dir.is_absolute() {
        root_dir.clone()
    } else {
        let cwd = env::current_dir().expect("Cannot get CWD");
        cwd.join(&root_dir)
    };

    let proj = served.canonicalize().unwrap_or(served.clone());

    if !proj.exists() {
        eprintln!("Error: root directory not found at {:?}", proj);
        std::process::exit(1);
    }

    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).expect("Cannot bind");
    eprintln!("[md_server_rs] http://{}  serving {:?}", addr, proj);
    let p = proj.clone();
    let s = served.clone();
    for stream in listener.incoming() {
        if let Ok(st) = stream {
            let r = s.clone();
            let j = p.clone();
            let b = base_path.clone();
            thread::spawn(move || handle(st, &r, &j, &b));
        }
    }
}

struct Req {
    path: String, query: HashMap<String, String>, body: String,
}

fn parse(s: &mut TcpStream) -> Option<Req> {
    let mut raw = [0u8; 16384];
    let n = s.read(&mut raw).ok().filter(|&n| n > 0)?;
    let req = String::from_utf8_lossy(&raw[..n]);
    let lines: Vec<&str> = req.lines().collect();
    if lines.is_empty() { return None; }
    let parts: Vec<&str> = lines[0].split_whitespace().collect();
    if parts.len() < 2 { return None; }
    let path = parts[1].to_string();
    let mut q = HashMap::new();
    let mut body = String::new();
    let mut in_body = false;
    for line in &lines[1..] {
        if in_body { body.push_str(line); body.push('\n'); }
        if line.trim().is_empty() { in_body = true; }
    }
    // parse query
    let clean_path = if let Some((p, qs)) = path.split_once('?') {
        for pair in qs.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                q.insert(k.to_string(), v.to_string());
            }
        }
        p.to_string()
    } else {
        path.clone()
    };
    Some(Req { path: clean_path, query: q, body })
}

fn handle(mut s: TcpStream, served: &Path, proj: &Path, base_path: &str) {
    let Some(req) = parse(&mut s) else { return };
    let path_raw = url_decode(&req.path.trim_start_matches('/')).to_string();
    // strip base_path prefix for routing (e.g. /docs/?dir=xxx → /?dir=xxx)
    let bp = base_path.trim_start_matches('/');
    let path = if !bp.is_empty() && path_raw.starts_with(bp) {
        path_raw[bp.len()..].to_string()
    } else {
        path_raw
    };
    let path = path.trim_start_matches('/').to_string();
    let q = req.query;
    let body = req.body;

    // route
    if path.is_empty() {
        let nav = nav_html(served, proj, &q, base_path);
        let cr = cur_root(served, proj, &q);
        let title = cr.file_name().and_then(|n| n.to_str()).unwrap_or("docs");
        let html = HTML_TPL.replace("{title}", title)
            .replace("{sidebar_w}", "240px")
            .replace("{nav}", &nav)
            .replace("{content}", "");
        send(&mut s, 200, "text/html;charset=utf-8", html.as_bytes());
    } else if is_supported(Path::new(&path)) {
        // try find and render
        let cr = cur_root(served, proj, &q);
        let fp = cr.join(&path);
        let content = if fp.is_file() {
            fs::read_to_string(&fp).ok()
        } else {
            // recursive search
            fs::read_dir(served).ok().and_then(|_| {
                walk(served, &path).and_then(|p| fs::read_to_string(&p).ok())
            })
        };
        if let Some(text) = content {
            let nav = nav_html(served, proj, &q, base_path);
            let cr = cur_root(served, proj, &q);
            let dir_title = cr.file_name().and_then(|n| n.to_str()).unwrap_or("docs");
            let fname = fp.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            let ext = fp.extension().and_then(|x| x.to_str()).unwrap_or("");
            let content_html = if ext == "md" {
                let mut opts = Options::empty();
                opts.insert(Options::ENABLE_TABLES);
                opts.insert(Options::ENABLE_FOOTNOTES);
                opts.insert(Options::ENABLE_STRIKETHROUGH);
                opts.insert(Options::ENABLE_TASKLISTS);
                let parser = Parser::new_ext(&text, opts);
                let mut html_buf = String::new();
                html::push_html(&mut html_buf, parser);
                let processed = process_md(&html_buf);
                format!("<article>{}</article>", processed)
            } else {
                // code files: display as syntax-highlighted code block
                format!(
                    "<article><h1>{}</h1><pre><code class=\"language-{}\">{}</code></pre></article>",
                    esc(fname),
                    esc(ext),
                    esc(&text)
                )
            };
            let html = HTML_TPL.replace("{title}", &format!("{} &mdash; {}", esc(fname), dir_title))
                .replace("{sidebar_w}", "240px")
                .replace("{nav}", &nav)
                .replace("{content}", &content_html);
            send(&mut s, 200, "text/html;charset=utf-8", html.as_bytes());
        } else {
            send_404(&mut s);
        }
    } else {
        send_404(&mut s);
    }
}

fn nav_html(served: &Path, proj: &Path, q: &HashMap<String, String>, base_path: &str) -> String {
    let cr = cur_root(served, proj, q);
    let mut p: Vec<String> = Vec::new();
    p.push(format!("<h3>{}</h3>", esc(cr.file_name().and_then(|n| n.to_str()).unwrap_or("?"))));
    // parent button: allow going up to proj
    if cr != proj {
        if let Some(parent) = cr.parent() {
            let dp = rel_p(parent, served, proj);
            let name = parent.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            p.push(format!("<a href='{}/?dir={}' class='parent-btn'>&#8593; 上一级: {}/</a>", base_path, esc(&dp), esc(name)));
        }
    }
    // subdirs
    if let Ok(entries) = fs::read_dir(&cr) {
        let mut subs: Vec<_> = entries.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).collect();
        subs.sort_by(|a, b| {
            let an = a.file_name().to_string_lossy().to_lowercase();
            let bn = b.file_name().to_string_lossy().to_lowercase();
            let a_is_chinese = an.chars().any(|c| c as u32 > 0x4E00);
            let b_is_chinese = bn.chars().any(|c| c as u32 > 0x4E00);
            if a_is_chinese && !b_is_chinese { std::cmp::Ordering::Greater }
            else if !a_is_chinese && b_is_chinese { std::cmp::Ordering::Less }
            else { an.cmp(&bn) }
        });
        if !subs.is_empty() {
            p.push("<h3>&#128193; 目录</h3>".into());
            for e in &subs {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') { continue; }
                let dp = rel_p(&e.path(), served, proj);
                let url_dir = url_encode(&dp);
                p.push(format!("<a href='{}/?dir={}'>&#128193; {}/</a>", base_path, url_dir, esc(&name)));
            }
        }
    }
    // supported files
    if let Ok(entries) = fs::read_dir(&cr) {
        let mut files: Vec<_> = entries.filter_map(|e| e.ok())
            .filter(|e| e.path().is_file() && is_supported(&e.path()))
            .collect();
        files.sort_by(|a, b| {
            let an = a.file_name().to_string_lossy().to_lowercase();
            let bn = b.file_name().to_string_lossy().to_lowercase();
            an.cmp(&bn)
        });
        if !files.is_empty() {
            p.push("<h3>&#128196; 文件</h3>".into());
            for e in &files {
                let name = e.file_name().to_string_lossy().to_string();
                let dp = rel_p(&cr, served, proj);
                let qs = if dp.is_empty() { String::new() } else { format!("?dir={}", url_encode(&dp)) };
                p.push(format!("<a href='{base_path}/{}{}'>{}</a>", url_encode(&name), qs, esc(&name)));
            }
        }
    }
    p.join("\n")
}

fn cur_root(served: &Path, proj: &Path, q: &HashMap<String, String>) -> PathBuf {
    let dir_raw = q.get("dir").map(|s| s.as_str()).unwrap_or("");
    let dir = if dir_raw.is_empty() { dir_raw } else { &url_decode(dir_raw) };
    if dir.is_empty() || dir == "." { return served.to_path_buf(); }
    let resolved = served.join(dir).canonicalize().unwrap_or_else(|_| served.to_path_buf());
    if resolved.starts_with(proj) { resolved } else { proj.to_path_buf() }
}

fn rel_p(target: &Path, served: &Path, proj: &Path) -> String {
    let t = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
    let b = served.canonicalize().unwrap_or_else(|_| served.to_path_buf());
    if t == b { return String::new(); }
    if let Ok(rel) = t.strip_prefix(&b) {
        rel.to_string_lossy().replace('\\', "/")
    } else {
        // above served dir
        let t_abs = t.to_string_lossy().to_string();
        let b_abs = b.to_string_lossy().to_string();
        let mut td: Vec<&str> = t_abs.split(&['\\', '/']).filter(|s| !s.is_empty()).collect();
        let mut bd: Vec<&str> = b_abs.split(&['\\', '/']).filter(|s| !s.is_empty()).collect();
        // find common prefix
        let mut i = 0;
        while i < td.len() && i < bd.len() && td[i] == bd[i] { i += 1; }
        let mut result = String::new();
        for _ in i..bd.len() { result.push_str("../"); }
        for j in i..td.len() { result.push_str(td[j]); if j < td.len() - 1 { result.push('/'); } }
        result
    }
}

fn walk(dir: &Path, name: &str) -> Option<PathBuf> {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                if let Some(found) = walk(&p, name) { return Some(found); }
            } else if p.is_file() && p.file_name().and_then(|n| n.to_str()) == Some(name) {
                return Some(p);
            }
        }
    }
    None
}

fn process_md(html: &str) -> String {
    // KaTeX 在客户端通过 auto-render 处理 $...$ 和 $$...$$
    // 服务端不做任何转换，直接透传
    html.to_string()
}

fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut b = [0u8; 3];
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i+1] as char).to_digit(16).unwrap_or(0) as u8;
            let lo = (bytes[i+2] as char).to_digit(16).unwrap_or(0) as u8;
            b[0] = (hi << 4) | lo;
            // try utf8 decode: 1-3 bytes
            let n = if b[0] & 0x80 == 0 { 1 }
                else if b[0] & 0xE0 == 0xC0 { 2 }
                else if b[0] & 0xF0 == 0xE0 { 3 }
                else { 1 };
            if n > 1 {
                for j in 1..n {
                    if i + j*3 + 2 < bytes.len() && bytes[i + j*3] == b'%' {
                        let hi2 = (bytes[i+1+j*3] as char).to_digit(16).unwrap_or(0) as u8;
                        let lo2 = (bytes[i+2+j*3] as char).to_digit(16).unwrap_or(0) as u8;
                        b[j] = (hi2 << 4) | lo2;
                    } else { return s.to_string(); }
                }
            }
            out.push_str(std::str::from_utf8(&b[..n]).unwrap_or("?"));
            i += n * 3;
        } else if bytes[i] == b'+' {
            out.push(' ');
            i += 1;
        } else {
            // raw non-ASCII byte: use lossy conversion
            let raw = &bytes[i..];
            let (ch, n) = if raw.len() >= 3 && (raw[0] & 0xF0) == 0xE0
                && (raw[1] & 0xC0) == 0x80 && (raw[2] & 0xC0) == 0x80 {
                // 3-byte UTF-8
                let c = std::str::from_utf8(&raw[..3]).unwrap_or("?");
                (c.to_string(), 3)
            } else if raw.len() >= 2 && (raw[0] & 0xE0) == 0xC0
                && (raw[1] & 0xC0) == 0x80 {
                let c = std::str::from_utf8(&raw[..2]).unwrap_or("?");
                (c.to_string(), 2)
            } else {
                (std::str::from_utf8(&raw[..1]).unwrap_or("?").to_string(), 1)
            };
            out.push_str(&ch);
            i += n;
        }
    }
    out
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b'/' => out.push('/'),  // keep path separators
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace("'", "&#x27;")
}

fn send(s: &mut TcpStream, status: u16, ct: &str, data: &[u8]) {
    let sl = match status { 200 => "200 OK", 404 => "404 Not Found", _ => "200 OK" };
    let h = format!("HTTP/1.0 {}\r\nContent-Type:{}\r\nContent-Length:{}\r\nCache-Control:no-store\r\nAccess-Control-Allow-Origin:*\r\n\r\n", sl, ct, data.len());
    let _ = s.write_all(h.as_bytes());
    let _ = s.write_all(data);
}

fn send_404(s: &mut TcpStream) {
    send(s, 404, "text/plain;charset=utf-8", b"File not found");
}
