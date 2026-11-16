//! Offline, read-only OFD page preview. This is not signature verification.
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::path::Path;
use std::sync::OnceLock;

fn script_url() -> &'static str {
    static SCRIPT: OnceLock<String> = OnceLock::new();
    SCRIPT.get_or_init(|| format!("data:text/javascript;base64,{}", STANDARD.encode(include_bytes!("../assets/ofd-viewer/ofdjs.js"))))
}

pub fn prepare(path: &Path) -> Result<(String, String)> {
    let bytes = crate::store::read_bounded(path, 16 * 1024 * 1024).context("OFD is too large for the layout preview")?;
    let (_, names) = crate::docx_preview::validated_archive(&bytes)?;
    ensure!(names.contains("OFD.xml"), "OFD structure is incomplete");
    let text = fdl_core::research::preview_ofd_bytes(path, &bytes)?;
    let data = STANDARD.encode(bytes);
    let html = format!(r#"<style>html,body{{display:block!important;overflow:auto!important;height:auto!important;margin:0!important;padding:0!important;background:#eee!important}}#ofd-controls{{position:sticky;top:0;background:#fff;padding:8px;z-index:2}}#ofd-warning{{color:#8b3000;margin:5px 0}}canvas{{display:block;margin:10px auto;background:white;max-width:100%;height:auto;box-shadow:0 1px 5px #888}}</style>
<div id="ofd-controls"><strong>OFD 只读预览 / Read-only preview</strong><div id="ofd-warning">版式可能不完整，印章可能缺失，签名未验证；请以专业阅读器和原件为准。 / Layout or seals may be missing; signatures are not verified.</div><button id="prev">上一页 / Previous</button> <span id="page">…</span> <button id="next">下一页 / Next</button><div id="diagnostics" role="status"></div></div><canvas id="ofd-canvas"></canvas>
<script type="module">import {{getDocument}} from '{script}';
const status=document.getElementById('diagnostics'),pageLabel=document.getElementById('page'),canvas=document.getElementById('ofd-canvas'),prev=document.getElementById('prev'),next=document.getElementById('next');
try{{const bytes=Uint8Array.from(atob('{data}'),c=>c.charCodeAt(0));const doc=await getDocument(bytes);if(!doc.numPages||doc.numPages>10000)throw Error('Unsupported page count');let current=1,busy=false;
async function draw(){{if(busy)return;busy=true;prev.disabled=next.disabled=true;try{{const page=await doc.getPage(current),viewport=page.getViewport({{scale:1}});if(!Number.isFinite(viewport.width)||!Number.isFinite(viewport.height)||viewport.width<=0||viewport.height<=0||viewport.width>4096||viewport.height>4096||viewport.width*viewport.height>16000000)throw Error('Page dimensions exceed preview limit');canvas.width=Math.ceil(viewport.width);canvas.height=Math.ceil(viewport.height);await page.render({{canvasContext:canvas.getContext('2d'),viewport}}).promise;pageLabel.textContent=current+' / '+doc.numPages;status.textContent=(doc.diagnostics||[]).map(d=>d.message).join(' · ');canvas.dataset.ready='true';}}catch(e){{status.textContent='页面无法显示 / Page preview failed: '+String(e.message||e);}}finally{{prev.disabled=current<=1;next.disabled=current>=doc.numPages;busy=false;}}}}
prev.onclick=()=>{{if(!busy&&current>1){{current--;draw();}}}};next.onclick=()=>{{if(!busy&&current<doc.numPages){{current++;draw();}}}};await draw();
}}catch(e){{status.textContent='版式无法显示，请查看下方文字 / Layout preview failed; see text below: '+String(e.message||e);}}
</script>"#, script=script_url());
    Ok((text, html))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_is_offline_bounded_and_preserves_original() {
        let root = std::env::temp_dir().join(format!("markdownx-ofd-preview-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("demo.ofd");
        let bytes = include_bytes!("../MarkdownX-Demo/版式文书示例.ofd");
        std::fs::write(&path, bytes).unwrap();
        let (text, html) = prepare(&path).unwrap();
        assert!(text.contains("版式文书演示") && text.contains("[Page 2]"));
        assert!(html.contains("签名未验证") && html.contains("data:text/javascript;base64,"));
        assert!(!html.contains(&path.to_string_lossy().to_string()));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_dir_all(root).unwrap();
    }
}
