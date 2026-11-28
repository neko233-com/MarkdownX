package pdf

import (
	"encoding/binary"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strconv"

	"github.com/signintech/gopdf"
	"golang.org/x/image/font"
	"golang.org/x/image/font/gofont/gobold"
	"golang.org/x/image/font/gofont/gobolditalic"
	"golang.org/x/image/font/gofont/goitalic"
	"golang.org/x/image/font/gofont/gomono"
	"golang.org/x/image/font/gofont/goregular"
	"golang.org/x/image/font/sfnt"
	"golang.org/x/image/math/fixed"
)

type weight int

const (
	regular weight = iota
	bold
	italic
	boldItalic
	mono
)

type face struct {
	name string
	font *sfnt.Font
}

type fontSet struct {
	pdf      *gopdf.GoPdf
	buffer   sfnt.Buffer
	faces    map[weight][]*face
	files    map[string]*face
	missing  map[weight]int
	sequence int
}

var goFaces = [...][]byte{goregular.TTF, gobold.TTF, goitalic.TTF, gobolditalic.TTF, gomono.TTF}

func (f *fontSet) add(data []byte) (*face, error) {
	parsed, err := sfnt.Parse(data)
	if err != nil {
		return nil, err
	}
	name := "MarkdownXFont" + strconv.Itoa(f.sequence)
	if err := f.pdf.AddTTFFontData(name, data); err != nil {
		return nil, err
	}
	f.sequence++
	return &face{name, parsed}, nil
}

func (f *fontSet) selectGlyph(style weight, r rune, size float64) (*face, float64, error) {
	if len(f.faces[style]) == 0 {
		base, err := f.add(goFaces[style])
		if err != nil {
			return nil, 0, err
		}
		f.faces[style] = []*face{base}
	}
	lookup := func(candidate *face) (float64, bool) {
		glyph, err := candidate.font.GlyphIndex(&f.buffer, r)
		if err != nil || glyph == 0 {
			return 0, false
		}
		// Use the unhinted font units. gopdf places text in points.
		em := int(candidate.font.UnitsPerEm())
		units, err := candidate.font.GlyphAdvance(&f.buffer, glyph, fixed.I(em), font.HintingNone)
		return float64(units) / (64 * float64(em)) * size, err == nil
	}
	for _, candidate := range f.faces[style] {
		if advance, ok := lookup(candidate); ok {
			return candidate, advance, nil
		}
	}
	paths := fallbackPaths(style)
	for f.missing[style] < len(paths) {
		path := paths[f.missing[style]]
		f.missing[style]++
		candidate, cached := f.files[path]
		if !cached {
			candidate = f.load(path)
			f.files[path] = candidate
		}
		if candidate == nil {
			continue
		}
		f.faces[style] = append(f.faces[style], candidate)
		if advance, ok := lookup(candidate); ok {
			return candidate, advance, nil
		}
	}
	return nil, 0, fmt.Errorf("找不到可用于导出的字形 %q（U+%04X）", r, r)
}

func (f *fontSet) load(path string) *face {
	if info, err := os.Stat(path); err != nil || !info.Mode().IsRegular() || info.Size() > 64<<20 {
		return nil
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return nil
	}
	ttf, err := embeddableTrueType(data)
	if err != nil {
		return nil
	}
	face, _ := f.add(ttf)
	return face
}

func fallbackPaths(style weight) []string {
	strong := style == bold || style == boldItalic
	var paths []string
	join := func(dir string, names ...string) {
		for _, name := range names {
			paths = append(paths, filepath.Join(dir, name))
		}
	}
	switch runtime.GOOS {
	case "windows":
		root := os.Getenv("WINDIR")
		if !filepath.IsAbs(root) {
			break
		}
		dir := filepath.Join(root, "Fonts")
		if strong {
			join(dir, "Dengb.ttf", "msyhbd.ttc")
		}
		join(dir, "Deng.ttf", "msyh.ttc", "simhei.ttf", "NotoSansSC-VF.ttf", "arial.ttf", "seguisym.ttf", "seguiemj.ttf", "SimsunExtG.ttf")
	case "darwin":
		if strong {
			join("/System/Library/Fonts", "STHeiti Medium.ttc")
		}
		join("/System/Library/Fonts", "PingFang.ttc", "Hiragino Sans GB.ttc", "STHeiti Light.ttc")
		join("/System/Library/Fonts/Supplemental", "Songti.ttc", "Arial.ttf", "Arial Unicode.ttf", "Apple Symbols.ttf")
		join("/Library/Fonts", "NotoSansSC-Regular.ttf", "NotoSansSC-VF.ttf")
		if home, err := os.UserHomeDir(); err == nil {
			join(filepath.Join(home, "Library", "Fonts"), "NotoSansSC-Regular.ttf", "NotoSansSC-VF.ttf")
		}
	}
	return paths
}

// gopdf subsets TrueType faces. A collection shares tables using file-relative
// offsets, so make a standalone face before giving it to the writer.
func embeddableTrueType(data []byte) ([]byte, error) {
	fail := errors.New("font is not a subsettable TrueType face")
	if len(data) < 12 {
		return nil, fail
	}
	standalone := string(data[:4]) != "ttcf"
	base := 0
	if !standalone {
		if len(data) < 16 || binary.BigEndian.Uint32(data[8:12]) == 0 {
			return nil, fail
		}
		base = int(binary.BigEndian.Uint32(data[12:16]))
	}
	if base > len(data)-12 || binary.BigEndian.Uint32(data[base:base+4]) != 0x00010000 {
		return nil, fail
	}
	count := int(binary.BigEndian.Uint16(data[base+4 : base+6]))
	headerLen := 12 + count*16
	if count == 0 || headerLen > len(data)-base {
		return nil, fail
	}
	var font []byte
	if !standalone {
		font = append(font, data[base:base+headerLen]...)
	}
	var glyf, rights bool
	var head int
	for i := 12; i < headerLen; i += 16 {
		entry := data[base+i : base+i+16]
		start := int(binary.BigEndian.Uint32(entry[8:12]))
		length := int(binary.BigEndian.Uint32(entry[12:16]))
		if start > len(data) || length > len(data)-start {
			return nil, fail
		}
		switch string(entry[:4]) {
		case "glyf":
			glyf = length > 0
		case "OS/2":
			if length < 10 {
				return nil, fail
			}
			flags := binary.BigEndian.Uint16(data[start+8 : start+10])
			rights = flags&0x0302 == 0
		case "head":
			if length < 12 {
				return nil, fail
			}
			head = len(font)
		}
		if !standalone {
			if len(font)+length > 64<<20 {
				return nil, fail
			}
			binary.BigEndian.PutUint32(font[i+8:i+12], uint32(len(font)))
			font = append(font, data[start:start+length]...)
			font = append(font, make([]byte, (-len(font))&3)...)
		}
	}
	if !glyf || !rights {
		return nil, fail
	}
	if standalone {
		return data, nil
	}
	if head == 0 {
		return nil, fail
	}
	binary.BigEndian.PutUint32(font[head+8:head+12], 0)
	var checksum uint32
	for i := 0; i < len(font); i += 4 {
		checksum += binary.BigEndian.Uint32(font[i : i+4])
	}
	binary.BigEndian.PutUint32(font[head+8:head+12], 0xB1B0AFBA-checksum)
	return font, nil
}
