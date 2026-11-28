package pdf

import (
	"bytes"
	"encoding/binary"
	"regexp"
	"strings"
	"testing"

	"github.com/neko233/markdownx/internal/doc"
	"golang.org/x/image/font/gofont/goregular"
	"golang.org/x/image/font/sfnt"
)

func TestPDFPaperBreaksAndDestinations(t *testing.T) {
	d := doc.ReadSource("[toc]\n\n# Opening\n\nVisit [other page](#destination) or [the site](https://example.org).\n\n" +
		`<div style="page-break-after:always"></div>` + "\n\n# Destination\n\nA note[^n].\n\n[^n]: Second page note.\n")
	opts := Defaults()
	opts.Paper, opts.Landscape = "Letter", true
	var out bytes.Buffer
	if err := Write(&out, d, opts); err != nil {
		t.Fatal(err)
	}
	s := out.String()
	for _, want := range []string{"%PDF-", "/Outlines", "/Subtype /Link", "/URI (https://example.org)", "/ToUnicode", "/FontFile2"} {
		if !strings.Contains(s, want) {
			t.Errorf("PDF is missing %q", want)
		}
	}
	if match := regexp.MustCompile(`/Count\s+2\b`).FindString(s); match == "" ||
		!regexp.MustCompile(`/MediaBox\s*\[\s*0[^ ]*\s+0[^ ]*\s+792[^ ]*\s+612[^ ]*\s*\]`).MatchString(s) {
		t.Fatal("forced page break or landscape Letter page dimensions were not written")
	}
	pages := regexp.MustCompile(`(?s)/Type\s*/Page\b[^>]*`).FindAllString(s, -1)
	if len(pages) != 2 {
		t.Fatalf("page objects = %d", len(pages))
	}
	for i, page := range pages {
		if !strings.Contains(page, "/Annots") {
			t.Errorf("page %d lost its link annotations", i+1)
		}
	}
}

func TestPDFPaginatesAnOversizedTableRow(t *testing.T) {
	const header = "| Name | Value |\n| --- | --- |\n"
	d := doc.ReadSource(header + "| Details | " + strings.Repeat("An entry with several words. ", 530) + " |\n")
	opts := Defaults()
	opts.Paper, opts.MarginMM = "A5", 12
	var out bytes.Buffer
	if err := Write(&out, d, opts); err != nil {
		t.Fatal(err)
	}
	pages := regexp.MustCompile(`/Type\s*/Page\b`).FindAll(out.Bytes(), -1)
	if len(pages) < 4 || len(pages) > 14 {
		t.Fatalf("oversized table cell used %d pages", len(pages))
	}
}

func TestPDFRejectsUnusablePageGeometryBeforeWriting(t *testing.T) {
	var out bytes.Buffer
	if err := Write(&out, doc.ReadSource("# Test\n"), Options{Paper: "A5", MarginMM: 130}); err == nil || out.Len() != 0 {
		t.Fatalf("invalid geometry gave %v and %d bytes", err, out.Len())
	}
}

func TestUseSubsettableCollectionFont(t *testing.T) {
	// A collection's shared tables are relative to the entire collection.
	const prefix = 16
	data := append(make([]byte, prefix), goregular.TTF...)
	copy(data, "ttcf")
	binary.BigEndian.PutUint32(data[4:8], 0x00010000)
	binary.BigEndian.PutUint32(data[8:12], 1)
	binary.BigEndian.PutUint32(data[12:16], prefix)
	count := int(binary.BigEndian.Uint16(data[prefix+4 : prefix+6]))
	var rights int
	for i := 0; i < count; i++ {
		record := prefix + 12 + i*16
		at := binary.BigEndian.Uint32(data[record+8:record+12]) + prefix
		binary.BigEndian.PutUint32(data[record+8:record+12], at)
		if string(data[record:record+4]) == "OS/2" {
			rights = int(at) + 8
		}
	}
	font, err := embeddableTrueType(data)
	if err != nil {
		t.Fatal(err)
	}
	parsed, err := sfnt.Parse(font)
	if err != nil {
		t.Fatal(err)
	}
	if glyph, err := parsed.GlyphIndex(nil, 'A'); err != nil || glyph == 0 {
		t.Fatalf("collection lost its character map: %v", err)
	}
	if rights < prefix {
		t.Fatal("font fixture has no embedding permissions table")
	}
	binary.BigEndian.PutUint16(data[rights:rights+2], 0x100)
	if _, err := embeddableTrueType(data); err == nil {
		t.Fatal("accepted a font flagged as not subsettable")
	}
}
