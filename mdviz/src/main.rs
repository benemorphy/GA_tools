// mdviz v0.4 — 侧边栏导航 + 按钮工具栏 + Webview
#![windows_subsystem = "windows"]

use clap::Parser;
use pulldown_cmark::{html, Options, Parser as MdParser};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::mpsc;
use std::thread;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "0")]
    port: u16,
    #[arg(long, default_value = "")]
    dir: String,
}

fn find_free_port(start: u16, end: u16) -> u16 {
    for port in start..=end {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() { return port; }
    }
    20000
}

fn main() {
    let args = Args::parse();
    let port = if args.port == 0 { find_free_port(20000, 20100) } else { args.port };
    let root = args.dir.clone();
    let addr = format!("127.0.0.1:{}", port);
    let url = format!("http://{}/", addr);
    let listener = TcpListener::bind(&addr).expect("Failed to bind");
    println!("md_server_rs running on {}", url);
    let srv_url = url.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => { let r = root.clone(); std::thread::spawn(move || handle_client(stream, &r)); }
                _ => { std::thread::sleep(std::time::Duration::from_millis(10)); }
            }
        }
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    open_webview(&srv_url);
}

fn handle_client(mut stream: TcpStream, root_dir: &str) {
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf).unwrap_or(0);
    if n == 0 { return; }
    let req = String::from_utf8_lossy(&buf[..n]);
    let (method, raw_path, query_string) = {
        let parts: Vec<&str> = req.splitn(3, ' ').collect();
        let uri = parts.get(1).copied().unwrap_or("/");
        let (p, q) = match uri.split_once('?') {
            Some((path_part, qs)) => (path_part, Some(qs.to_string())),
            None => (uri, None),
        };
        (parts.first().unwrap_or(&"GET").to_string(), url_decode(p), q)
    };
    let path = raw_path;
    match method.as_str() {
        "GET" | "HEAD" => {
            if path == "/" || path.starts_with("/list/") { handle_list(&mut stream, &path, root_dir); }
            else if path == "/favicon.ico" { respond(&mut stream, 404, "Not Found", ""); }
            else if path.starts_with("/pdf/") { handle_pdf(&mut stream, &path); }
            else if path == "/navigate" { handle_navigate(&mut stream, &query_string); }
            else { handle_file(&mut stream, &path); }
        }
        _ => respond(&mut stream, 405, "Method Not Allowed", ""),
    }
}

fn respond(stream: &mut TcpStream, status: u16, reason: &str, body: &str) {
    let resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-cache, no-store, must-revalidate\r\nPragma: no-cache\r\nExpires: 0\r\nConnection: close\r\n\r\n{}",
        status, reason, body.len(), body
    );
    let _ = stream.write(resp.as_bytes());
}

fn respond_redirect(stream: &mut TcpStream, location: &str) {
    let resp = format!(
        "HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        location
    );
    let _ = stream.write(resp.as_bytes());
}

fn url_decode(s: &str) -> String {
    let mut bytes = Vec::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hi = chars.next().and_then(|c| c.to_digit(16)).unwrap_or(0);
            let lo = chars.next().and_then(|c| c.to_digit(16)).unwrap_or(0);
            bytes.push((hi * 16 + lo) as u8);
        } else {
            let mut buf = [0u8; 4];
            let encoded = c.encode_utf8(&mut buf);
            bytes.extend_from_slice(encoded.as_bytes());
        }
    }
    String::from_utf8(bytes.clone()).unwrap_or_else(|_| String::from_utf8_lossy(&bytes).to_string())
}

fn url_encode_path(path: &str) -> String {
    let mut result = String::new();
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b':' | b'-' | b'_' | b'\\' => {
                result.push(b as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", b));
            }
        }
    }
    result
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn get_all_drives() -> Vec<String> {
    let mut drives = Vec::new();
    for letter in 'A'..='Z' {
        let path = format!("{}:\\", letter);
        if Path::new(&path).exists() { drives.push(format!("{}:", letter)); }
    }
    drives
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 { format!("{} B", bytes) }
    else if bytes < 1024 * 1024 { format!("{:.1} KB", bytes as f64 / 1024.0) }
    else { format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)) }
}

fn render_sidebar(current_path: &str) -> String {
    let mut html = String::from("<nav class='sidebar'>");
    
    if current_path.is_empty() || current_path == "/" {
        for d in get_all_drives() {
            let drive_path = format!("{}:\\", &d[..1]);
            let label = format!("本地磁盘 ({})", d);
            html.push_str(&format!(
                "<div class='entry'><a href='/list/{0}:/'><span class='drive-icon'>&#x1F4BF;</span> {1}</a></div>",
                d.trim_end_matches(':'), label
            ));
        }
    } else {
        // 返回上级目录
        if !current_path.ends_with(':') {
            let parent = Path::new(&current_path.replace('/', "\\")).parent()
                .and_then(|p| p.to_str())
                .map(|p| p.replace('\\', "/"))
                .unwrap_or_default();
            if !parent.is_empty() {
                let parent_url = format!("/list/{}/", parent);
                html.push_str(&format!("<div class='entry up'><a href='{}'>⬆ ..</a></div>", parent_url));
            }
        }
        let fs_path = if current_path.ends_with(':') { format!("{}:\\", &current_path[..1]) } else { current_path.replace('/', "\\") };
        let dir = Path::new(&fs_path);
        if dir.exists() && dir.is_dir() {
            if let Ok(entries) = fs::read_dir(dir) {
                let mut items: Vec<(String, bool)> = Vec::new();
                let supported = ["md", "json", "py", "html", "log"];
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !name.starts_with('.') {
                        if let Ok(ft) = entry.file_type() {
                            if ft.is_dir() {
                                items.push((name, true));
                            } else if let Some(ext) = name.rsplit('.').next() {
                                if supported.contains(&ext) {
                                    items.push((name, false));
                                }
                            }
                        }
                    }
                }
                items.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.to_lowercase().cmp(&b.0.to_lowercase())));
                for (name, is_dir) in &items {
                    let icon = if *is_dir { "📁" } else { "📄" };
                    let enc_name = url_encode_path(name);
                    let url = if current_path.is_empty() {
                        if *is_dir { format!("/list/{}/", enc_name) } else { format!("/{}", enc_name) }
                    } else {
                        let base = current_path.trim_end_matches('/');
                        let enc_base = url_encode_path(base);
                        if *is_dir { format!("/list/{}/{}/", enc_base, enc_name) } else { format!("/{}/{}", enc_base, enc_name) }
                    };
                    html.push_str(&format!("<div class='entry'><a href='{}'>{} {}</a></div>", url, icon, html_escape(name)));
                }
            }
        }
    }
    html.push_str("</nav>");
    html
}

fn handle_navigate(stream: &mut TcpStream, query_string: &Option<String>) {
    let target = query_string.as_ref()
        .and_then(|qs| {
            qs.split('&')
                .find(|pair| pair.starts_with("path="))
                .and_then(|pair| pair.split_once('='))
                .map(|(_, v)| url_decode(v))
        })
        .unwrap_or_default();
    if target.is_empty() {
        respond(stream, 400, "Bad Request", "<h1>400</h1><p>缺少 path 参数</p>");
        return;
    }
    let normalized = target.replace('\\', "/").trim_end_matches('/').to_string();
    let fs_path = if normalized.ends_with(':') {
        format!("{}:\\", &normalized[..1])
    } else {
        normalized.replace('/', "\\")
    };
    let path_obj = Path::new(&fs_path);
    if path_obj.is_dir() {
        respond_redirect(stream, &format!("/list/{}/", url_encode_path(&normalized)));
    } else if path_obj.is_file() {
        respond_redirect(stream, &format!("/{}", url_encode_path(&normalized)));
    } else {
        if let Some(parent) = path_obj.parent() {
            if parent.exists() && parent.is_dir() {
                let parent_str = parent.to_str().unwrap_or("").replace('\\', "/");
                respond_redirect(stream, &format!("/list/{}/", url_encode_path(&parent_str)));
                return;
            }
        }
        respond(stream, 404, "Not Found", &format!("<div style='text-align:center;padding:40px;font-family:sans-serif;'><h1>404</h1><p>路径不存在: {}</p><p><a href='javascript:history.back()' style='color:#e94560;text-decoration:none;font-size:16px;'>↩ 返回</a></p></div>", html_escape(&target)));
    }
}

fn handle_list(stream: &mut TcpStream, raw_path: &str, root_dir: &str) {
    let show_all = raw_path == "/" || raw_path == "/list/";
    let current_path = if show_all { String::new() } else {
        raw_path.strip_prefix("/list/").unwrap_or("").trim_end_matches('/').to_string()
    };
    let sidebar = render_sidebar(&current_path);
    let path_esc = html_escape(&current_path);
    let mut content = String::new();
    if show_all {
        content.push_str("<div class='empty-preview'>");
        content.push_str("<div class='empty-icon'>📖</div>");
        content.push_str("<div class='empty-text'>请在左侧选择文件</div>");
        content.push_str("<div class='empty-hint'>支持 Markdown / 代码 / 文本文件预览</div>");
        content.push_str("</div>");
    } else {
        content.push_str("<div class='empty-preview'>");
        content.push_str("<div class='empty-icon'>📖</div>");
        content.push_str("<div class='empty-text'>请在左侧选择文件</div>");
        content.push_str("<div class='empty-hint'>支持 Markdown / 代码 / 文本文件预览</div>");
        content.push_str("</div>");
    }
    let html = format!(
        r#"<!DOCTYPE html><html lang="zh-CN"><head><meta charset="utf-8">
        <title>{path_esc}</title>
        <style>
        * {{margin:0;padding:0;box-sizing:border-box;}}
        body {{display:flex;height:100vh;font:15px/1.6 -apple-system,sans-serif;background:var(--bg);color:var(--fg);}}
        .sidebar {{width:260px;min-width:260px;background:var(--sidebar-bg);overflow-y:auto;border-right:1px solid var(--sidebar-bg);padding:8px;}}
        .entry a {{display:block;padding:4px 8px;border-radius:4px;text-decoration:none;color:var(--fg);font-size:13px;}}
        .entry a:hover {{background:var(--sidebar-hover);}}
        .entry.up {{margin-bottom:6px;border-bottom:1px solid var(--sidebar-bg);padding-bottom:6px;}}
        .main {{flex:1;display:flex;flex-direction:column;overflow:hidden;}}
        .toolbar {{display:flex;align-items:center;gap:8px;padding:6px 12px;background:var(--toolbar-bg);border-bottom:1px solid var(--sidebar-bg);flex-wrap:wrap;}}
        .toolbar button {{padding:4px 10px;border:1px solid var(--sidebar-bg);border-radius:4px;background:var(--toolbar-bg);color:var(--fg);cursor:pointer;font-size:13px;}}
        .toolbar button:hover {{background:var(--sidebar-hover);}}
        .theme-group {{display:flex;gap:2px;}}
        .theme-group button {{width:26px;height:26px;padding:0;border-radius:50%;font-size:10px;line-height:26px;text-align:center;border:1px solid var(--sidebar-bg);}}
        .nav-input {{display:flex;flex:1;gap:4px;}}
        .nav-input input {{flex:1;padding:4px 8px;border:1px solid var(--sidebar-bg);border-radius:4px;font-size:13px;background:var(--toolbar-bg);color:var(--fg);}}
        .content {{flex:1;overflow-y:auto;padding:20px 30px;max-width:900px;}}
        .content h1 {{font-size:22px;margin:0 0 12px;}}
        .content h2 {{font-size:18px;margin:16px 0 8px;border-bottom:1px solid var(--sidebar-bg);padding-bottom:4px;}}
        .content h3 {{font-size:16px;margin:12px 0 6px;}}
        .content p,li {{margin:6px 0;line-height:1.7;}}
        .content pre {{background:var(--code-bg);padding:12px;border-radius:6px;overflow-x:auto;font-size:13px;}}
        .content code {{font-family:'Cascadia Code','JetBrains Mono',monospace;font-size:13px;}}
        .content table {{border-collapse:collapse;width:100%;margin:12px 0;}}
        .content th,.content td {{border:1px solid var(--table-border);padding:6px 10px;text-align:left;}}
        .content th {{background:var(--table-header);font-weight:600;}}
        .content img {{max-width:100%;border-radius:4px;}}
        .content a {{color:var(--accent);}}
        .content blockquote {{border-left:3px solid var(--accent);padding:4px 12px;margin:8px 0;background:var(--code-bg);border-radius:0 4px 4px 0;}}
        .empty-preview {{display:flex;flex-direction:column;align-items:center;justify-content:center;height:60vh;color:var(--muted);}}
        .empty-icon {{font-size:64px;margin-bottom:16px;opacity:0.5;}}
        .empty-text {{font-size:18px;color:var(--muted);margin-bottom:8px;}}
        .empty-hint {{font-size:13px;color:var(--muted);}}
        .footer {{padding:8px 30px;font-size:12px;color:var(--muted);border-top:1px solid var(--sidebar-bg);}}
        @media print {{.sidebar,.toolbar,.footer {{display:none!important;}}}}
        </style>
        <script>
        var themes={{light:{{bg:'#fafafa',fg:'#2c3e50',side:'#eef0f4',accent:'#e94560',muted:'#999',code:'#f5f5f5',tbl:'#ddd',thdr:'#eee'}},
                        dark:{{bg:'#1e1e2e',fg:'#cdd6f4',side:'#181825',accent:'#89b4fa',muted:'#6c7086',code:'#313244',tbl:'#45475a',thdr:'#313244'}},
                        sepia:{{bg:'#fbf3e8',fg:'#5b4636',side:'#efe4d0',accent:'#aa6f3d',muted:'#b8956a',code:'#e8dcc8',tbl:'#d4c5a9',thdr:'#e0d0b8'}},
                        sepiadark:{{bg:'#2b2416',fg:'#d4b88c',side:'#1f1a10',accent:'#e6a65d',muted:'#8a7355',code:'#3a3020',tbl:'#4a3d28',thdr:'#3a3020'}}}};
        function exportPDF(){{
            var path=window.location.pathname;
            var active=document.querySelector('.entry.active a');
            if(active)path=active.getAttribute('href');
            var pdfUrl='/pdf'+path;
            var a=document.createElement('a');
            a.href=pdfUrl; a.download='';
            document.body.appendChild(a); a.click();
            document.body.removeChild(a);
        }}
        function exportHTML(){{
            var c=document.querySelector('.content');
            if(!c)c=document.body;
            var clone=document.documentElement.cloneNode(true);
            var toRm=clone.querySelector('.sidebar,.toolbar,.footer');
            if(toRm)toRm.remove();
            var h='<!DOCTYPE html>'+clone.outerHTML;
            var b=new Blob([h],{{type:'text/html'}});
            var u=URL.createObjectURL(b);var a=document.createElement('a');
            a.href=u;a.download=document.title.replace(/[<>:"/\\\\|?*]/g,'_')+'.html';
            document.body.appendChild(a);a.click();
            document.body.removeChild(a);URL.revokeObjectURL(u);
        }}
        function exportDOC(){{
            var c=document.querySelector('.content');
            if(!c)c=document.body;
            var t=localStorage.getItem('theme')||'light';
            var th=themes[t];
            var s='<style>body{{font:14px/1.6 -apple-system,sans-serif;padding:20px 30px;max-width:900px;margin:0 auto;background:'+th.bg+';color:'+th.fg+';}}'+
                'pre,code{{font-family:monospace;font-size:13px;}}'+
                'pre{{background:'+th.code+';padding:12px;border-radius:4px;overflow-x:auto;}}'+
                'table{{border-collapse:collapse;width:100%;margin:12px 0;}}'+
                'th,td{{border:1px solid '+th.tbl+';padding:6px 10px;text-align:left;}}'+
                'th{{background:'+th.thdr+';font-weight:600;}}'+
                'a{{color:'+th.accent+';text-decoration:none;}}'+
                'img{{max-width:100%;border-radius:4px;}}'+
                '</style>';
            var h='<html><meta charset=utf-8><head>'+s+'</head><body>'+c.innerHTML+'</body></html>';
            var b=new Blob([h],{{type:'application/msword'}});
            var u=URL.createObjectURL(b);var a=document.createElement('a');
            a.href=u;a.download=document.title.replace(/[<>:"/\\|?*]/g,'_')+'.doc';
            document.body.appendChild(a);a.click();
            document.body.removeChild(a);URL.revokeObjectURL(u);
        }}
        function applyTheme(){{
            var t=localStorage.getItem('theme')||'light';
            var th=themes[t]||themes.light;
            var r=document.documentElement.style;
            r.setProperty('--bg',th.bg);r.setProperty('--fg',th.fg);
            r.setProperty('--sidebar-bg',th.side);
            r.setProperty('--sidebar-hover','color-mix(in srgb,'+th.side+' 80%, '+th.fg+')');
            r.setProperty('--accent',th.accent);r.setProperty('--muted',th.muted);
            r.setProperty('--code-bg',th.code);r.setProperty('--table-border',th.tbl);
            r.setProperty('--table-header',th.thdr);
        }}
        applyTheme();
        </script>
        </head><body>
        {sidebar}
        <div class='main'>
        <div class='toolbar'>
            <button onclick='exportPDF()' title='导出为PDF(完整页面)'>📥 PDF</button>
            <button onclick='exportHTML()' title='导出为HTML文件'>📄 HTML</button>
            <button onclick='exportDOC()' title='导出为Word(.doc)'>📄 DOC</button>
            <div class='nav-input'>
                <input id='navPath' type='text' placeholder='输入路径如 C:\\' value='{path_esc}' onkeydown="if(event.key==='Enter'){{window.location.href='/navigate?path='+encodeURIComponent(this.value)}}"/>
                <button onclick="window.location.href='/navigate?path='+encodeURIComponent(document.getElementById('navPath').value)">跳转</button>
            </div>
            <div class='theme-group'>
                <button onclick="localStorage.setItem('theme','light');applyTheme()" style='background:#fafafa;color:#2c3e50;border:1px solid #ddd;' title='亮色'>☀</button>
                <button onclick="localStorage.setItem('theme','dark');applyTheme()" style='background:#1a1a2e;color:#e6e6e6;' title='暗色'>🌙</button>
                <button onclick="localStorage.setItem('theme','sepia');applyTheme()" style='background:#efe4d0;color:#5b4636;' title='羊皮纸'>📜</button>
                <button onclick="localStorage.setItem('theme','sepiadark');applyTheme()" style='background:#1f1a10;color:#d4b88c;' title='暗羊皮纸'>🌃</button>
            </div>
        </div>
        <div class='content'>{content}</div>
        </div></body></html>"#,
        sidebar = sidebar, path_esc = path_esc, content = content
    );
    respond(stream, 200, "OK", &html);
}

fn handle_file(stream: &mut TcpStream, raw_path: &str) {
    let clean_path = raw_path.trim_start_matches('/');
    let fs_path = clean_path.replace('/', "\\");
    let file = Path::new(&fs_path);
    if !file.exists() || !file.is_file() {
        respond(stream, 404, "Not Found", &format!("<div style='text-align:center;padding:40px;font-family:sans-serif;'><h1>404</h1><p>文件不存在: {}</p><p><a href='javascript:history.back()' style='color:#e94560;text-decoration:none;font-size:16px;'>↩ 返回</a></p></div>", html_escape(&fs_path)));
        return;
    }
    let content = fs::read_to_string(file).unwrap_or_default();
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let body_html = if ext == "md" {
        let mut opts = Options::all();
        opts.insert(Options::ENABLE_TABLES);
        opts.insert(Options::ENABLE_FOOTNOTES);
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        opts.insert(Options::ENABLE_TASKLISTS);
        let parser = MdParser::new_ext(&content, opts);
        let mut html_out = String::new();
        html::push_html(&mut html_out, parser);
        html_out
    } else if ext == "html" {
        // 嵌入 sidebar 布局，内容通过 iframe srcdoc 隔离
        // 使用双引号包裹 srcdoc，html_escape 已将 " 转义为 &quot;，不会冲突
        format!(
            "<iframe srcdoc=\"{}\" style='width:100%;height:calc(100vh - 44px);border:none;background:#fff;' sandbox='allow-scripts'></iframe>",
            html_escape(&content)
        )
    } else if ext == "json" || ext == "py" || ext == "log" || ext == "txt" || ext == "rs" || ext == "toml" || ext == "yaml" || ext == "yml" || ext == "js" || ext == "css" {
        let escaped = html_escape(&content);
        let list_link = file.parent().and_then(|p| p.to_str()).unwrap_or("").replace('\\', "/");
        let enc_link = url_encode_path(&list_link);
        format!(
            "<div class='nav'><a href='/list/{0}/'>↩ 返回目录</a></div>
            <pre><code class='language-{1}'>{2}</code></pre>",
            enc_link, ext, escaped
        )
    } else {
        respond(stream, 403, "Forbidden", "<h1>403</h1><p>不支持的文件类型</p>");
        return;
    };
    let file_name = file.file_name().and_then(|x| x.to_str()).unwrap_or("file");
    let file_dir = file.parent().and_then(|p| p.to_str()).unwrap_or("");
    let list_link = file_dir.replace('\\', "/");
    let current_path = file_dir.to_string();
    let sidebar = render_sidebar(&current_path);
    let footer = format!(
        "<div class='footer'>{} &middot; {}</div>",
        file_name, format_size(file.metadata().map(|m| m.len()).unwrap_or(0))
    );
    let html = format!(
        r#"<!DOCTYPE html><html lang="zh-CN"><head><meta charset="utf-8">
        <title>{title}</title>
        <style>
        * {{margin:0;padding:0;box-sizing:border-box;}}
        body {{display:flex;height:100vh;font:15px/1.6 -apple-system,sans-serif;background:var(--bg);color:var(--fg);}}
        .sidebar {{width:260px;min-width:260px;background:var(--sidebar-bg);overflow-y:auto;border-right:1px solid var(--sidebar-bg);padding:8px;}}
        .sidebar-header {{padding:8px;font-weight:bold;font-size:14px;color:var(--fg);border-bottom:1px solid var(--sidebar-bg);margin-bottom:8px;}}
        .entry a {{display:block;padding:4px 8px;border-radius:4px;text-decoration:none;color:var(--fg);font-size:13px;}}
        .entry a:hover {{background:var(--sidebar-hover);}}
        .entry.up {{margin-bottom:6px;border-bottom:1px solid var(--sidebar-bg);padding-bottom:6px;}}
        .main {{flex:1;display:flex;flex-direction:column;overflow:hidden;}}
        .toolbar {{display:flex;align-items:center;gap:8px;padding:6px 12px;background:var(--toolbar-bg);border-bottom:1px solid var(--sidebar-bg);flex-wrap:wrap;}}
        .toolbar button {{padding:4px 10px;border:1px solid var(--sidebar-bg);border-radius:4px;background:var(--toolbar-bg);color:var(--fg);cursor:pointer;font-size:13px;}}
        .toolbar button:hover {{background:var(--sidebar-hover);}}
        .theme-group {{display:flex;gap:2px;}}
        .theme-group button {{width:26px;height:26px;padding:0;border-radius:50%;font-size:10px;line-height:26px;text-align:center;border:1px solid var(--sidebar-bg);}}
        .nav-input {{display:flex;flex:1;gap:4px;}}
        .nav-input input {{flex:1;padding:4px 8px;border:1px solid var(--sidebar-bg);border-radius:4px;font-size:13px;background:var(--toolbar-bg);color:var(--fg);}}
        .content {{flex:1;overflow-y:auto;padding:20px 30px;max-width:900px;}}
        .content h1 {{font-size:22px;margin:0 0 12px;}}
        .content h2 {{font-size:18px;margin:16px 0 8px;border-bottom:1px solid var(--sidebar-bg);padding-bottom:4px;}}
        .content h3 {{font-size:16px;margin:12px 0 6px;}}
        .content p,li {{margin:6px 0;line-height:1.7;}}
        .content pre {{background:var(--code-bg);padding:12px;border-radius:6px;overflow-x:auto;font-size:13px;}}
        .content code {{font-family:'Cascadia Code','JetBrains Mono',monospace;font-size:13px;}}
        .content table {{border-collapse:collapse;width:100%;margin:12px 0;}}
        .content th,.content td {{border:1px solid var(--table-border);padding:6px 10px;text-align:left;}}
        .content th {{background:var(--table-header);font-weight:600;}}
        .content img {{max-width:100%;border-radius:4px;}}
        .content a {{color:var(--accent);}}
        .content blockquote {{border-left:3px solid var(--accent);padding:4px 12px;margin:8px 0;background:var(--code-bg);border-radius:0 4px 4px 0;}}
        .nav a {{color:var(--accent);text-decoration:none;}}
        .footer {{padding:8px 30px;font-size:12px;color:var(--muted);border-top:1px solid var(--sidebar-bg);}}
        @media print {{.sidebar,.toolbar,.footer {{display:none!important;}}}}
        </style>
        <script>
        var themes={{light:{{bg:'#fafafa',fg:'#2c3e50',side:'#eef0f4',accent:'#e94560',muted:'#999',code:'#f5f5f5',tbl:'#ddd',thdr:'#eee'}},
                        dark:{{bg:'#1e1e2e',fg:'#cdd6f4',side:'#181825',accent:'#89b4fa',muted:'#6c7086',code:'#313244',tbl:'#45475a',thdr:'#313244'}},
                        sepia:{{bg:'#fbf3e8',fg:'#5b4636',side:'#efe4d0',accent:'#aa6f3d',muted:'#b8956a',code:'#e8dcc8',tbl:'#d4c5a9',thdr:'#e0d0b8'}},
                        sepiadark:{{bg:'#2b2416',fg:'#d4b88c',side:'#1f1a10',accent:'#e6a65d',muted:'#8a7355',code:'#3a3020',tbl:'#4a3d28',thdr:'#3a3020'}}}};
        function exportPDF(){{
            var path=window.location.pathname;
            var active=document.querySelector('.entry.active a');
            if(active)path=active.getAttribute('href');
            var pdfUrl='/pdf'+path;
            var a=document.createElement('a');
            a.href=pdfUrl; a.download='';
            document.body.appendChild(a); a.click();
            document.body.removeChild(a);
        }}
        function exportHTML(){{
            var c=document.querySelector('.content');
            if(!c)c=document.body;
            var clone=document.documentElement.cloneNode(true);
            var sidebar=clone.querySelector('.sidebar');
            var toolbar=clone.querySelector('.toolbar');
            var footer=clone.querySelector('.footer');
            if(sidebar)sidebar.remove();if(toolbar)toolbar.remove();if(footer)footer.remove();
            var h='<!DOCTYPE html>'+clone.outerHTML;
            var b=new Blob([h],{{type:'text/html'}});
            var u=URL.createObjectURL(b);var a=document.createElement('a');
            a.href=u;a.download=document.title.replace(/[<>:"/\\\\|?*]/g,'_')+'.html';
            document.body.appendChild(a);a.click();
            document.body.removeChild(a);URL.revokeObjectURL(u);
        }}
        function exportDOC(){{
            var c=document.querySelector('.content');
            if(!c)c=document.body;
            var t=localStorage.getItem('theme')||'light';
            var th=themes[t];
            var s='<style>body{{font:14px/1.6 -apple-system,sans-serif;padding:20px 30px;max-width:900px;margin:0 auto;background:'+th.bg+';color:'+th.fg+';}}'+
                'pre,code{{font-family:monospace;font-size:13px;}}'+
                'pre{{background:'+th.code+';padding:12px;border-radius:4px;overflow-x:auto;}}'+
                'table{{border-collapse:collapse;width:100%;margin:12px 0;}}'+
                'th,td{{border:1px solid '+th.tbl+';padding:6px 10px;text-align:left;}}'+
                'th{{background:'+th.thdr+';font-weight:600;}}'+
                'a{{color:'+th.accent+';text-decoration:none;}}'+
                'img{{max-width:100%;border-radius:4px;}}'+
                '</style>';
            var h='<html><meta charset=utf-8><head>'+s+'</head><body>'+c.innerHTML+'</body></html>';
            var b=new Blob([h],{{type:'application/msword'}});
            var u=URL.createObjectURL(b);var a=document.createElement('a');
            a.href=u;a.download=document.title.replace(/[<>:"/\\|?*]/g,'_')+'.doc';
            document.body.appendChild(a);a.click();
            document.body.removeChild(a);URL.revokeObjectURL(u);
        }}
        function applyTheme(){{
            var t=localStorage.getItem('theme')||'light';
            var th=themes[t]||themes.light;
            var r=document.documentElement.style;
            r.setProperty('--bg',th.bg);r.setProperty('--fg',th.fg);
            r.setProperty('--sidebar-bg',th.side);
            r.setProperty('--sidebar-hover','color-mix(in srgb,'+th.side+' 80%, '+th.fg+')');
            r.setProperty('--accent',th.accent);r.setProperty('--muted',th.muted);
            r.setProperty('--code-bg',th.code);r.setProperty('--table-border',th.tbl);
            r.setProperty('--table-header',th.thdr);
        }}
        applyTheme();
        </script>
        </head><body>
        {sidebar}
        <div class='main'>
        <div class='toolbar'>
            <button onclick='exportPDF()' title='导出为PDF(完整页面)'>📥 PDF</button>
            <button onclick='exportHTML()' title='导出为HTML文件'>📄 HTML</button>
            <button onclick='exportDOC()' title='导出为Word(.doc)'>📄 DOC</button>
            <div class='nav-input'>
                <input id='navPath' type='text' placeholder='输入路径如 C:\\' value='{path_esc}' onkeydown="if(event.key==='Enter'){{window.location.href='/navigate?path='+encodeURIComponent(this.value)}}"/>
                <button onclick="window.location.href='/navigate?path='+encodeURIComponent(document.getElementById('navPath').value)">跳转</button>
            </div>
            <div class='theme-group'>
                <button onclick="localStorage.setItem('theme','light');applyTheme()" style='background:#fafafa;color:#2c3e50;border:1px solid #ddd;' title='亮色'>☀</button>
                <button onclick="localStorage.setItem('theme','dark');applyTheme()" style='background:#1a1a2e;color:#e6e6e6;' title='暗色'>🌙</button>
                <button onclick="localStorage.setItem('theme','sepia');applyTheme()" style='background:#efe4d0;color:#5b4636;' title='羊皮纸'>📜</button>
                <button onclick="localStorage.setItem('theme','sepiadark');applyTheme()" style='background:#1f1a10;color:#d4b88c;' title='暗羊皮纸'>🌃</button>
            </div>
        </div>
        <div class='content'>{body_html}</div>
        {footer}
        </div></body></html>"#,
        title = html_escape(file_name),
        sidebar = sidebar,
        path_esc = html_escape(&list_link),
        body_html = body_html,
        footer = footer,
    );
    respond(stream, 200, "OK", &html);
}

fn handle_pdf(stream: &mut TcpStream, raw_path: &str) {
    let clean = raw_path.strip_prefix("/pdf/").unwrap_or("").split('?').next().unwrap_or("");
    let fs_path = clean.replace("/", "\\");
    let file = Path::new(&fs_path);
    if !file.exists() || !file.is_file() {
        respond(stream, 404, "Not Found", "<h1>404</h1><p>File not found</p>");
        return;
    }
    let content = fs::read_to_string(file).unwrap_or_default();
    let body_html = if file.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase() == "md" {
        let mut opts = Options::all();
        opts.insert(Options::ENABLE_TABLES);
        opts.insert(Options::ENABLE_FOOTNOTES);
        opts.insert(Options::ENABLE_STRIKETHROUGH);
        opts.insert(Options::ENABLE_TASKLISTS);
        let parser = MdParser::new_ext(&content, opts);
        let mut html_out = String::new();
        html::push_html(&mut html_out, parser);
        html_out
    } else {
        html_escape(&content)
    };
    let title = file.file_name().and_then(|n| n.to_str()).unwrap_or("document");
    let full_html = format!(
        "<!DOCTYPE html><html><head><meta charset=utf-8><title>{}</title>\
        <style>body{{font:12pt/1.6 'Times New Roman',serif;color:#000;padding:30px;max-width:210mm;margin:auto;}}\
        pre{{background:#f5f5f5;padding:12px;border-radius:4px;overflow-x:auto;white-space:pre-wrap;}}\
        table{{border-collapse:collapse;width:100%;margin:12px 0;}}th,td{{border:1px solid #999;padding:6px 10px;}}\
        img{{max-width:100%;}}h1,h2,h3{{page-break-after:avoid;}}\
        @media print{{@page{{margin:15mm 20mm;}}}}</style></head><body>{}</body></html>",
        html_escape(title), body_html
    );
    let tmp_dir = std::env::temp_dir();
    let tmp_html = tmp_dir.join(format!("md2pdf_{}.html", std::process::id()));
    let parent_dir = file.parent().and_then(|p| p.to_str()).unwrap_or(".");
    let stem = file.file_stem().and_then(|x| x.to_str()).unwrap_or("output");
    let tmp_pdf = std::path::PathBuf::from(parent_dir).join(format!("{}.pdf", stem));
    let _ = fs::write(&tmp_html, &full_html);
    let chrome = r"C:\Program Files\Google\Chrome\Application\chrome.exe";
    use std::process::Command;
    let result = Command::new(chrome)
        .arg("--headless")
        .arg("--disable-gpu")
        .arg("--no-margins")
        .arg(format!("--print-to-pdf={}", tmp_pdf.display()))
        .arg(tmp_html.to_str().unwrap_or(""))
        .output();
    let _ = fs::remove_file(&tmp_html);
    if let Ok(output) = result {
        if output.status.success() {
            if let Ok(pdf_bytes) = fs::read(&tmp_pdf) {
                let _ = fs::remove_file(&tmp_pdf);
                let fname = html_escape(title);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"{}.pdf\"\r\nConnection: close\r\n\r\n",
                    pdf_bytes.len(), fname
                );
                let _ = stream.write(header.as_bytes());
                let _ = stream.write_all(&pdf_bytes);
                return;
            }
        }
    }
    respond(stream, 500, "Internal Server Error", "<h1>500</h1><p>PDF generation failed</p>");
}

fn open_webview(url: &str) {
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::window::WindowBuilder;
    use wry::WebViewBuilder;
    
    let el = EventLoopBuilder::new().build();
    let window = WindowBuilder::new()
        .with_title("MD Server - 文档预览")
        .with_inner_size(tao::dpi::LogicalSize::new(1280, 800))
        .build(&el)
        .unwrap();
    let _webview = WebViewBuilder::new(&window)
        .with_url(url)
        .build();
    
    el.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let tao::event::Event::WindowEvent {
            event: tao::event::WindowEvent::CloseRequested,
            ..
        } = event {
            *control_flow = ControlFlow::Exit;
        }
    });
}
