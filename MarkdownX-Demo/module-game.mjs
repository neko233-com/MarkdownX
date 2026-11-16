import { mountSlate } from './module-helper.mjs';
const { initial } = await import('./module-label.mjs');
mountSlate(document.getElementById('slate'), initial);
