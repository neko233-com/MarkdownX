package convert

import (
	"archive/zip"
	"bytes"
	"testing"
)

type xlsxZipPart struct {
	name string
	body string
}

// xlsxBuild assembles a minimal .xlsx archive in memory.
func xlsxBuild(t *testing.T, parts ...xlsxZipPart) []byte {
	t.Helper()
	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	for _, p := range parts {
		w, err := zw.Create(p.name)
		if err != nil {
			t.Fatalf("create %s: %v", p.name, err)
		}
		if _, err := w.Write([]byte(p.body)); err != nil {
			t.Fatalf("write %s: %v", p.name, err)
		}
	}
	if err := zw.Close(); err != nil {
		t.Fatalf("close zip: %v", err)
	}
	return buf.Bytes()
}

const xlsxWorkbookXML = `<?xml version="1.0" encoding="UTF-8"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets>
<sheet name="Data" sheetId="1" r:id="rId1"/>
<sheet name="Second" sheetId="2" r:id="rId2"/>
</sheets>
</workbook>`

const xlsxSharedXML = `<?xml version="1.0" encoding="UTF-8"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="3">
<si><t>Ann</t></si>
<si><r><t>T</t></r><r><t>om</t></r></si>
<si><t>9001</t></si>
</sst>`

const xlsxSheet1XML = `<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1"><c r="A1" t="inlineStr"><is><t>Name</t></is></c><c r="B1" t="inlineStr"><is><t>Score</t></is></c><c r="C1" t="inlineStr"><is><t>Note</t></is></c></row>
<row r="2"><c r="A2" t="s"><v>0</v></c><c r="B2"><v>42</v></c><c r="C2" t="inlineStr"><is><t>a|b</t></is></c></row>
<row r="3"><c r="A3" t="s"><v>1</v></c><c r="C3" t="inlineStr"><is><t>line1&#10;line2</t></is></c></row>
<row r="4"/>
<row r="5"><c r="D5" t="b"><v>1</v></c></row>
</sheetData>
</worksheet>`

const xlsxSheet2XML = `<?xml version="1.0" encoding="UTF-8"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetData>
<row r="1"><c r="A1" t="inlineStr"><is><t>Key</t></is></c><c r="B1" t="s"><v>2</v></c></row>
<row r="2"><c r="A2"><v>7</v></c><c r="B2" t="b"><v>0</v></c></row>
</sheetData>
</worksheet>`

const xlsxWant = "## Data\n" +
	"| Name | Score | Note |  |\n" +
	"| --- | --- | --- | --- |\n" +
	"| Ann | 42 | a\\|b |  |\n" +
	"| Tom |  | line1<br>line2 |  |\n" +
	"|  |  |  | TRUE |\n" +
	"\n" +
	"## Second\n" +
	"| Key | 9001 |\n" +
	"| --- | --- |\n" +
	"| 7 | FALSE |\n"

func TestXlsx(t *testing.T) {
	workbook := xlsxBuild(t,
		xlsxZipPart{"xl/workbook.xml", xlsxWorkbookXML},
		xlsxZipPart{"xl/sharedStrings.xml", xlsxSharedXML},
		xlsxZipPart{"xl/worksheets/sheet1.xml", xlsxSheet1XML},
		xlsxZipPart{"xl/worksheets/sheet2.xml", xlsxSheet2XML},
	)
	empty := xlsxBuild(t,
		xlsxZipPart{"xl/workbook.xml", `<?xml version="1.0"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheets><sheet name="Empty" sheetId="1" r:id="rId1"/></sheets>
</workbook>`},
	)

	tests := []struct {
		name    string
		data    []byte
		want    string
		wantErr bool
	}{
		{name: "two sheets", data: workbook, want: xlsxWant},
		{name: "empty sheet keeps heading", data: empty, want: "## Empty\n"},
		{name: "not a zip", data: []byte("not a zip"), wantErr: true},
		{name: "missing workbook", data: xlsxBuild(t, xlsxZipPart{"xl/worksheets/sheet1.xml", xlsxSheet1XML}), wantErr: true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := Xlsx(tt.data)
			if tt.wantErr {
				if err == nil {
					t.Fatalf("Xlsx() error = nil, want error")
				}
				return
			}
			if err != nil {
				t.Fatalf("Xlsx() error = %v", err)
			}
			if got != tt.want {
				t.Errorf("Xlsx() =\n%q\nwant\n%q", got, tt.want)
			}
		})
	}
}
