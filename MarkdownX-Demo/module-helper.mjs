import { marker } from './module-label.mjs';

export function mountSlate(canvas, initial) {
  let count = initial;
  const note = document.createElement('p');
  const frame = new Image();
  frame.alt = '分镜预览';
  frame.width = 64;
  frame.src = new URL('storyboard-shots/02.svg', import.meta.url).href;
  canvas.insertAdjacentElement('afterend', note);
  note.insertAdjacentElement('afterend', frame);
  const render = () => { note.textContent = marker + count; };
  canvas.addEventListener('click', () => { count += 1; render(); });
  render();
}
