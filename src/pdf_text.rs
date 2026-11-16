//! Isolated, bounded PDF text extraction. The worker reads only from stdin and
//! writes only to stdout; it never opens or modifies the user's document.
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

const MAX_PDF: u64 = 256 * 1024 * 1024;
const MAX_PAGE_TEXT: usize = 1024 * 1024;
const MAX_TOTAL_TEXT: usize = 16 * 1024 * 1024;
const MAX_RESULT: u64 = 40 * 1024 * 1024;
const MAX_PAGES: usize = 10_000;

#[derive(Serialize, Deserialize)]
pub(crate) struct PdfText { pub pages: Vec<Option<String>> }

struct LimitedText { bytes: Vec<u8>, max: usize }
impl Write for LimitedText {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.max.saturating_sub(self.bytes.len()) { return Err(io::Error::other("PDF text limit exceeded")); }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

fn extract(bytes: &[u8]) -> Result<PdfText, String> {
    let mut doc = lopdf::Document::load_mem(bytes).map_err(|_| "PDF text cannot be read")?;
    if doc.is_encrypted() { doc.decrypt("").map_err(|_| "PDF text is encrypted")?; }
    let pages = doc.get_pages();
    if pages.len() > MAX_PAGES { return Err("PDF has too many pages".into()); }
    let mut result = Vec::with_capacity(pages.len());
    let mut total = 0usize;
    for page in pages.keys() {
        let mut writer = LimitedText { bytes: vec![], max: MAX_PAGE_TEXT.min(MAX_TOTAL_TEXT.saturating_sub(total)) };
        let completed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let stream: &mut dyn Write = &mut writer;
            let mut output = pdf_extract::PlainTextOutput::new(stream);
            pdf_extract::output_doc_page(&doc, &mut output, *page).is_ok()
        })).unwrap_or(false);
        let text = completed.then(|| String::from_utf8(writer.bytes).ok()).flatten().map(|mut text| {
            text.retain(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'));
            total += text.len();
            text
        });
        result.push(text);
    }
    Ok(PdfText { pages: result })
}

pub(crate) fn worker() -> Result<(), String> {
    let mut input = vec![];
    io::stdin().lock().take(MAX_PDF + 1).read_to_end(&mut input).map_err(|e| e.to_string())?;
    if input.len() as u64 > MAX_PDF { return Err("PDF is too large".into()); }
    let result = extract(&input)?;
    serde_json::to_writer(io::stdout().lock(), &result).map_err(|e| e.to_string())
}

pub(crate) struct WorkerJob(windows::Win32::Foundation::HANDLE);
impl Drop for WorkerJob { fn drop(&mut self) { let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.0) }; } }

pub(crate) fn constrain(child: &std::process::Child) -> Result<WorkerJob, String> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY, JobObjectExtendedLimitInformation};
    let job = WorkerJob(unsafe { CreateJobObjectW(None, None) }.map_err(|e| e.to_string())?);
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    limits.ProcessMemoryLimit = 768 * 1024 * 1024;
    unsafe {
        SetInformationJobObject(job.0, JobObjectExtendedLimitInformation, (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(), std::mem::size_of_val(&limits) as u32).map_err(|e| e.to_string())?;
        AssignProcessToJobObject(job.0, HANDLE(child.as_raw_handle())).map_err(|e| e.to_string())?;
    }
    Ok(job)
}

pub(crate) fn index(path: &Path, in_memory: Option<Arc<Vec<u8>>>, cancel: &Arc<AtomicBool>) -> Result<PdfText, String> {
    use std::os::windows::process::CommandExt;
    let bytes = if let Some(bytes) = in_memory {
        if bytes.len() as u64 > MAX_PDF { return Err("PDF is too large".into()); }
        bytes
    } else {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.len() > MAX_PDF { return Err("PDF is too large".into()); }
        let mut bytes = vec![];
        file.take(MAX_PDF + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_PDF { return Err("PDF is too large".into()); }
        Arc::new(bytes)
    };
    if cancel.load(Ordering::Relaxed) { return Err("cancelled".into()); }
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg("--internal-pdf-text-worker").creation_flags(0x08000000)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| e.to_string())?;
    let _job = match constrain(&child) {
        Ok(job) => job,
        Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error); }
    };
    let mut stdin = child.stdin.take().ok_or("PDF worker input unavailable")?;
    let stdout = child.stdout.take().ok_or("PDF worker output unavailable")?;
    let writer = std::thread::spawn(move || stdin.write_all(&bytes));
    let reader = std::thread::spawn(move || {
        let mut output = vec![];
        stdout.take(MAX_RESULT + 1).read_to_end(&mut output).map(|_| output)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if !cancel.load(Ordering::Relaxed) && started.elapsed() < Duration::from_secs(120) => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => { let _ = child.kill(); let _ = child.wait(); break Err("PDF text indexing stopped or timed out".to_string()); }
            Err(error) => { let _ = child.kill(); let _ = child.wait(); break Err(error.to_string()); }
        }
    };
    let written = writer.join().map_err(|_| "PDF worker input failed")?;
    let output = reader.join().map_err(|_| "PDF worker output failed")?.map_err(|e| e.to_string())?;
    if !status?.success() || written.is_err() { return Err("PDF text could not be extracted".into()); }
    if output.len() as u64 > MAX_RESULT { return Err("PDF text result is too large".into()); }
    serde_json::from_slice(&output).map_err(|_| "PDF text result is invalid".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn blank_pdf() -> Vec<u8> {
        let objects = ["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 160] /Resources << >> /Contents 4 0 R >>", "<< /Length 0 >>\nstream\n\nendstream"];
        let mut bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        for (i, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", i + 1, object).as_bytes());
        }
        let xref = bytes.len();
        bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes());
        for offset in &offsets[1..] { bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes()); }
        bytes.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", offsets.len()).as_bytes());
        bytes
    }

    #[test]
    fn extracts_the_chinese_demo_by_page_and_rejects_invalid_input() {
        let bytes = include_bytes!("../MarkdownX-Demo/论文阅读示例.pdf");
        let result = extract(bytes).unwrap();
        assert_eq!(result.pages.len(), 3);
        assert!(result.pages[0].as_deref().unwrap().contains("研究背景"));
        assert!(result.pages[1].as_deref().unwrap().contains("方法与观察"));
        assert!(result.pages[2].as_deref().unwrap().contains("参考资料"));
        let blank = extract(&blank_pdf()).unwrap();
        assert_eq!(blank.pages.len(), 1);
        assert!(blank.pages[0].as_deref().is_none_or(|text| text.trim().is_empty()));
        assert!(extract(b"not a PDF").is_err());
    }
}
