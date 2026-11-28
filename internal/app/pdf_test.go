package app

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/neko233/markdownx/internal/doc"
	"github.com/neko233/markdownx/internal/export/pdf"
)

func TestPDFDialogAndAtomicExportUseUnsavedSource(t *testing.T) {
	dir := isolateConfig(t)
	a := newTestApp("# Before\n")
	a.toggleSource()
	a.source.edit.Buffer.Set("# Unsaved PDF heading\n\n[Target](https://example.org/unsaved)\n")
	a.ExportPDF()
	tr := ui.NewTester(a.View, 920, 680)
	tr.Frame()
	if !tr.HasText("导出 PDF") || !tr.HasText("页边距 (mm)") {
		t.Fatal("the native PDF dialogue is missing")
	}
	a.pdfDialog.settings.Margin = "0.5"
	tr.Frame()
	if err := tr.Click("选择保存位置…"); err != nil {
		t.Fatal(err)
	}
	if a.pdfDialog.err == "" || a.pdfJob != nil {
		t.Fatal("invalid margins got past the settings dialogue")
	}
	opts := pdf.Defaults()
	path := filepath.Join(dir, "export.pdf")
	saveSamplePDF(t, a, path, a.currentDocument().Clone(), opts)
	data, err := os.ReadFile(path)
	if err != nil || !strings.Contains(string(data), "/URI (https://example.org/unsaved)") || a.source == nil {
		t.Fatalf("export did not use the unsaved source buffer: %v", err)
	}
}

func saveSamplePDF(t *testing.T, a *App, path string, d *doc.Document, opts pdf.Options) {
	t.Helper()
	a.startPDF(path, d, opts, false)
	select {
	case done := <-a.pdfJob:
		a.pdfJob <- done
		a.pollPDF()
		if done.err != nil {
			t.Fatal(done.err)
		}
	case <-time.After(20 * time.Second):
		t.Fatal("export did not finish")
	}
}

// TestPDFVisualSample writes a sample for checking with a PDF reader and renderer.
func TestPDFVisualSample(t *testing.T) {
	qa := os.Getenv("MDX_PDF_QA_DIR")
	if qa == "" {
		t.Skip("set MDX_PDF_QA_DIR to save the PDF sample")
	}
	dir := isolateConfig(t)
	const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="390" height="85" viewBox="0 0 390 85"><rect x="2" y="2" width="386" height="81" rx="12" fill="#e7eef4"/><path d="M31 61l63-21 61 14 68-31 63 8 63-14" fill="none" stroke="#245d8b" stroke-width="5"/></svg>`
	if err := os.WriteFile(filepath.Join(dir, "trend.svg"), []byte(svg), 0o600); err != nil {
		t.Fatal(err)
	}
	var source strings.Builder
	source.WriteString("[toc]\n\n# 中文导出检查\n\n这是一段可搜索、可复制的中文。这里包含 **粗体**、*斜体* 和 [网页][site]。🙂\n\n" +
		"从[目录](#详细记录)跳转到下一章，旁边是行内公式 $x^2+y^2$。脚注示例[^source]。\n\n" +
		"![趋势示意](trend.svg)\n\n> 核对段落间距、字体和换行。\n\n$$\n\\frac{1}{2}+\\sqrt{2}\n$$\n\n" +
		"```mermaid\nflowchart LR\n    A[Draft] --> B[Review] --> C[Print]\n```\n\n# 详细记录\n\n| 序号 | 中文明细 | 金额 |\n| ---: | --- | ---: |\n")
	for n := 1; n <= 35; n++ {
		source.WriteString("| " + itoa(n) + " | 用多行中文验证跨页表格。标点和长单词应保持清晰。 | 123.45 |\n")
	}
	source.WriteString("\n[site]: https://example.org/a?x=1&y=2 \"示例站点\"\n\n" +
		"[^source]: 脚注内容和返回链接。\n\n    第二段脚注正文。\n")
	opts := pdf.Defaults()
	opts.Title, opts.Header, opts.Media, opts.BreakAtH1 = "年度工作笔记", "${title}", pdfMedia(dir, true), true
	path := filepath.Join(qa, "layout-sample.pdf")
	saveSamplePDF(t, newTestApp(""), path, doc.ReadSource(source.String()), opts)
	t.Log(path)
}
