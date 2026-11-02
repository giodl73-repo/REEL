import init, { inspect_assembly } from './pkg/reel_assembly_workbench.js';
const started = performance.now();
await init();
postMessage({type: 'ready', initialization_ms: performance.now() - started});
onmessage = ({data}) => {
  try {
    const started = performance.now();
    const text = inspect_assembly(data.text, data.target, data.kind, data.pointer);
    postMessage({type: 'result', id: data.id, text, computation_ms: performance.now() - started});
  } catch (error) {
    postMessage({type: 'error', id: data.id, message: String(error)});
  }
};
