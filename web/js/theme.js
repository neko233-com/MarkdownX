// Keep a book's authored accent, adjusting only its displayed text contrast.
export function readableAccent(value, dark) {
  let hex = String(value || '').replace(/^#([\da-f])([\da-f])([\da-f])$/i, '#$1$1$2$2$3$3');
  if (!/^#[\da-f]{6}$/i.test(hex)) hex = '#0070f3';
  const channels = [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16));
  const luminance = rgb => rgb.reduce((sum, byte, i) => {
    const v = byte / 255;
    return sum + [0.2126, 0.7152, 0.0722][i] * (v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4);
  }, 0);
  const background = luminance(dark ? [23, 23, 23] : [250, 250, 250]);
  for (let step = 0; step <= 255; step++) {
    const rgb = channels.map(v => Math.round(v + ((dark ? 255 : 0) - v) * step / 255));
    const ink = luminance(rgb);
    if ((Math.max(ink, background) + .05) / (Math.min(ink, background) + .05) >= 4.5) {
      return '#' + rgb.map(v => v.toString(16).padStart(2, '0')).join('');
    }
  }
  return dark ? '#ededed' : '#171717';
}
