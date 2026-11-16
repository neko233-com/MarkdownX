// Self-contained book assets only: model dependencies never fetch remote URLs.
import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { OBJLoader } from 'three/addons/loaders/OBJLoader.js';
import { STLLoader } from 'three/addons/loaders/STLLoader.js';
import { assetURL } from './mdxb.js';

function decodeAsset(book, src) {
  const url = assetURL(book, src);
  const match = url.match(/^data:[a-z0-9.+-]+\/[a-z0-9.+-]+;base64,([a-z0-9+/=]+)$/i);
  if (!match) throw new Error('Model asset is not embedded');
  if (match[1].length > 24 * 1024 * 1024) throw new Error('Model asset is too large');
  const raw = atob(match[1]);
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return bytes.buffer;
}

function checkGltf(data, binary) {
  let json;
  if (binary) {
    const view = new DataView(data);
    if (data.byteLength < 20 || view.getUint32(0, true) !== 0x46546c67 || view.getUint32(4, true) !== 2 || view.getUint32(8, true) !== data.byteLength || view.getUint32(16, true) !== 0x4e4f534a) throw new Error('Invalid GLB header');
    const length = view.getUint32(12, true);
    if (length > data.byteLength - 20) throw new Error('Invalid GLB JSON');
    json = new TextDecoder().decode(new Uint8Array(data, 20, length)).trim();
  } else json = new TextDecoder().decode(data);
  const model = JSON.parse(json);
  if (!model || typeof model !== 'object' || !Array.isArray(model.accessors) || (model.nodes?.length || 0) > 100000) throw new Error('Invalid model structure');
  let total = 0;
  for (const accessor of model.accessors) {
    const count = accessor?.count;
    if (!Number.isSafeInteger(count) || count < 0 || count > 1500000) throw new Error('Model is too complex');
    total += count;
    if (total > 8000000) throw new Error('Model is too complex');
  }
}

function localManager(book, source) {
  const manager = new THREE.LoadingManager();
  manager.setURLModifier((value) => {
    if (/^(data:|blob:)/i.test(value)) return value;
    try {
      const root = new URL(source, 'https://markdownx.invalid/');
      const target = new URL(value, root);
      if (target.origin !== root.origin || target.search || target.hash) return 'data:application/octet-stream;base64,';
      const result = assetURL(book, target.pathname.slice(1));
      return /^data:/i.test(result) ? result : 'data:application/octet-stream;base64,';
    } catch { return 'data:application/octet-stream;base64,'; }
  });
  return manager;
}

export async function mountModel(container, source, book, lang) {
  const zh = lang === 'zh';
  const status = document.createElement('div');
  status.className = 'model-status';
  status.textContent = zh ? '正在加载模型…' : 'Loading model…';
  container.appendChild(status);
  const suffix = source.split(/[?#]/)[0].split('.').pop().toLowerCase();
  const data = decodeAsset(book, source);
  const manager = localManager(book, source);
  let object, animations = [];
  if (suffix === 'obj') {
    const text = new TextDecoder().decode(data);
    let vertices = 0, faces = 0;
    for (const line of text.split(/\r?\n/)) {
      if (/^\s*v\s/.test(line) && ++vertices > 1000000 || /^\s*f\s/.test(line) && ++faces > 500000) throw new Error('Model is too complex');
    }
    object = new OBJLoader(manager).parse(text);
  }
  else if (suffix === 'stl') {
    const geometry = new STLLoader(manager).parse(data);
    geometry.computeVertexNormals();
    object = new THREE.Mesh(geometry, new THREE.MeshStandardMaterial({ color: 0xb9c9d0, roughness: 0.72 }));
  } else if (suffix === 'glb' || suffix === 'gltf') {
    checkGltf(data, suffix === 'glb');
    const gltf = await new GLTFLoader(manager).parseAsync(suffix === 'gltf' ? new TextDecoder().decode(data) : data, '');
    object = gltf.scene;
    animations = gltf.animations || [];
  } else throw new Error('Unsupported model type');

  const bounds = new THREE.Box3().setFromObject(object);
  const size = bounds.getSize(new THREE.Vector3());
  const largest = Math.max(size.x, size.y, size.z);
  if (bounds.isEmpty() || !Number.isFinite(largest) || largest <= 0) throw new Error('Model has no visible geometry');
  const center = bounds.getCenter(new THREE.Vector3());
  object.position.sub(center);
  const model = new THREE.Group();
  model.add(object);
  model.scale.setScalar(2 / largest);
  const scene = new THREE.Scene();
  scene.add(model, new THREE.HemisphereLight(0xffffff, 0x657785, 2.2));
  const light = new THREE.DirectionalLight(0xffffff, 2.3);
  light.position.set(4, 6, 5);
  scene.add(light);
  const camera = new THREE.PerspectiveCamera(38, 1, 0.01, 1000);
  camera.position.set(3.4, 2.5, 4.2);
  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: 'low-power' });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  renderer.outputColorSpace = THREE.SRGBColorSpace;
  renderer.domElement.tabIndex = 0;
  renderer.domElement.setAttribute('aria-label', zh ? '三维模型预览' : '3D model preview');
  status.remove();
  container.prepend(renderer.domElement);
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = false;
  controls.minDistance = 1.6;
  controls.maxDistance = 16;
  controls.update();
  let stopped = false, frame = 0, mixer = null;
  const render = () => { if (!stopped) renderer.render(scene, camera); };
  controls.addEventListener('change', render);
  const resize = () => {
    const width = Math.max(1, container.clientWidth);
    const height = Math.max(120, container.clientHeight);
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
    renderer.setSize(width, height, false);
    render();
  };
  const observer = new ResizeObserver(resize);
  observer.observe(container);
  resize();
  if (animations.length) {
    mixer = new THREE.AnimationMixer(object);
    mixer.clipAction(animations[0]).play();
    let previous = 0;
    const tick = (time) => {
      if (stopped) return;
      if (previous && !document.hidden) { mixer.update(Math.min((time - previous) / 1000, 0.1)); render(); }
      previous = time;
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
  }
  const toolbar = document.createElement('div');
  toolbar.className = 'model-controls';
  const button = (label, action) => {
    const item = document.createElement('button');
    item.type = 'button'; item.textContent = label;
    item.addEventListener('click', () => { action(); render(); });
    toolbar.appendChild(item);
  };
  button(zh ? '向左旋转' : 'Rotate left', () => { model.rotation.y -= Math.PI / 12; });
  button(zh ? '向右旋转' : 'Rotate right', () => { model.rotation.y += Math.PI / 12; });
  button(zh ? '重置' : 'Reset', () => { model.rotation.y = 0; camera.position.set(3.4, 2.5, 4.2); controls.target.set(0, 0, 0); controls.update(); });
  container.appendChild(toolbar);
  return () => {
    stopped = true;
    cancelAnimationFrame(frame);
    observer.disconnect();
    controls.dispose();
    mixer?.stopAllAction();
    const disposed = new Set();
    object.traverse((item) => {
      if (item.geometry && !disposed.has(item.geometry)) { disposed.add(item.geometry); item.geometry.dispose(); }
      for (const material of Array.isArray(item.material) ? item.material : item.material ? [item.material] : []) {
        for (const value of Object.values(material)) if (value?.isTexture && !disposed.has(value)) { disposed.add(value); value.source?.data?.close?.(); value.dispose(); }
        if (!disposed.has(material)) { disposed.add(material); material.dispose(); }
      }
    });
    renderer.dispose();
    renderer.forceContextLoss();
  };
}
