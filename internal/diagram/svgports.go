package diagram

import (
	"bytes"
	"encoding/xml"
	"math"
	"regexp"
	"strconv"
	"strings"
)

type svgPoint struct{ x, y float64 }

type svgOutline struct {
	polygon  []svgPoint
	min, max svgPoint
	radius   float64
}

var edgePath = regexp.MustCompile(`(?m)^\s*<path d="(M[^"]+)" fill="none" stroke="[^"]*" stroke-width="`)
var pathPoint = regexp.MustCompile(`-?\d+(?:\.\d+)?,-?\d+(?:\.\d+)?`)

// The renderer routes flowchart edges to enclosing rectangles. Extend its
// orthogonal ends to the visible outlines of diamond and rounded nodes.
func attachFlowchartEdges(data []byte) []byte {
	shapes := flowchartOutlines(data)
	if len(shapes) == 0 {
		return data
	}
	var out bytes.Buffer
	at := 0
	for _, edge := range edgePath.FindAllSubmatchIndex(data, -1) {
		path := data[edge[2]:edge[3]]
		positions := pathPoint.FindAllIndex(path, -1)
		if len(positions) < 2 {
			continue
		}
		first, last := positions[0], positions[len(positions)-1]
		parse := func(p []int) svgPoint { return svgCoords(string(path[p[0]:p[1]])) }
		start, end := parse(first), parse(last)
		from := adjustPort(shapes, start, parse(positions[1]))
		to := adjustPort(shapes, end, parse(positions[len(positions)-2]))
		if from == start && to == end {
			continue
		}
		out.Write(data[at : edge[2]+first[0]])
		writePoint(&out, from)
		out.Write(path[first[1]:last[0]])
		writePoint(&out, to)
		at = edge[2] + last[1]
	}
	if at == 0 {
		return data
	}
	out.Write(data[at:])
	return out.Bytes()
}

func flowchartOutlines(data []byte) []svgOutline {
	var shapes []svgOutline
	d := xml.NewDecoder(bytes.NewReader(data))
	for {
		t, err := d.Token()
		if err != nil {
			return shapes
		}
		e, ok := t.(xml.StartElement)
		if !ok {
			continue
		}
		var o svgOutline
		switch e.Name.Local {
		case "polygon":
			for _, xy := range strings.Fields(svgAttr(e, "points")) {
				p := svgCoords(xy)
				if len(o.polygon) == 0 {
					o.min, o.max = p, p
				}
				o.min = svgPoint{min(o.min.x, p.x), min(o.min.y, p.y)}
				o.max = svgPoint{max(o.max.x, p.x), max(o.max.y, p.y)}
				o.polygon = append(o.polygon, p)
			}
			if len(o.polygon) < 3 {
				continue
			}
		case "circle":
			o.radius = svgNumber(svgAttr(e, "r"))
			cx, cy := svgNumber(svgAttr(e, "cx")), svgNumber(svgAttr(e, "cy"))
			o.min, o.max = svgPoint{cx - o.radius, cy - o.radius}, svgPoint{cx + o.radius, cy + o.radius}
		case "rect":
			if svgAttr(e, "stroke") == "" {
				continue
			}
			o.radius = svgNumber(svgAttr(e, "rx"))
			x, y := svgNumber(svgAttr(e, "x")), svgNumber(svgAttr(e, "y"))
			w, h := svgNumber(svgAttr(e, "width")), svgNumber(svgAttr(e, "height"))
			o.radius = min(w/2, h/2, o.radius)
			o.min, o.max = svgPoint{x, y}, svgPoint{x + w, y + h}
		default:
			continue
		}
		if len(o.polygon) != 0 || o.radius > 0 {
			shapes = append(shapes, o)
		}
	}
}

func adjustPort(shapes []svgOutline, p, outside svgPoint) svgPoint {
	const rounding = 0.05 // Coordinates are rounded to two decimal places.
	vertical := math.Abs(p.x-outside.x) < rounding
	if !vertical && math.Abs(p.y-outside.y) >= rounding {
		return p
	}
	axis := func(v svgPoint) (along, cross float64) {
		if vertical {
			return v.y, v.x
		}
		return v.x, v.y
	}
	start, offset := axis(p)
	neighbor, _ := axis(outside)
	if math.Abs(start-neighbor) < rounding {
		return p
	}
	for _, shape := range shapes {
		lo, near := axis(shape.min)
		hi, far := axis(shape.max)
		if offset < near-rounding || offset > far+rounding {
			continue
		}
		fromLow := neighbor < start
		face := hi
		if fromLow {
			face = lo
		}
		if math.Abs(start-face) >= rounding {
			continue
		}
		var hit float64
		if shape.radius > 0 {
			d := max(0, near+shape.radius-offset, offset-(far-shape.radius))
			inset := shape.radius - math.Sqrt(max(0, shape.radius*shape.radius-d*d))
			hit = hi - inset
			if fromLow {
				hit = lo + inset
			}
		} else {
			hit = polygonContact(shape.polygon, axis, offset, fromLow, face)
		}
		if vertical {
			p.y = hit
		} else {
			p.x = hit
		}
		return p
	}
	return p
}

func polygonContact(points []svgPoint, axis func(svgPoint) (float64, float64), offset float64, fromLow bool, face float64) float64 {
	var result float64
	found := false
	for i, p := range points {
		a, start := axis(p)
		b, end := axis(points[(i+1)%len(points)])
		if offset < min(start, end) || offset > max(start, end) || start == end {
			continue
		}
		hit := a + (b-a)*(offset-start)/(end-start)
		if !found || (fromLow && hit < result) || (!fromLow && hit > result) {
			result, found = hit, true
		}
	}
	if !found {
		return face
	}
	return result
}

func svgAttr(e xml.StartElement, name string) string {
	for _, a := range e.Attr {
		if a.Name.Local == name {
			return a.Value
		}
	}
	return ""
}

func svgNumber(s string) float64 { n, _ := strconv.ParseFloat(s, 64); return n }

func svgCoords(s string) svgPoint {
	x, y, _ := strings.Cut(s, ",")
	return svgPoint{svgNumber(x), svgNumber(y)}
}

func writePoint(b *bytes.Buffer, p svgPoint) {
	b.WriteString(strconv.FormatFloat(p.x, 'f', 2, 64))
	b.WriteByte(',')
	b.WriteString(strconv.FormatFloat(p.y, 'f', 2, 64))
}
