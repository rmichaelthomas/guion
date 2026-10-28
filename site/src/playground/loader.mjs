// Guion playground: compile one source file in WebAssembly, run it on the shared VM (browser ABI 1.0).
// Nothing here runs until the visitor presses Run. The functions are DOM-free so Node can test them.

export const LIMITS = Object.freeze({ sourceBytes: 65536, steps: 10000, outputBytes: 65536 });

const encoder = new TextEncoder();
const decoder = new TextDecoder();
const ABI = { major: 1, minor: 0 };

async function instantiate(bytes) {
  const { instance } = await WebAssembly.instantiate(bytes, {});
  if (!(instance.exports.memory instanceof WebAssembly.Memory)) throw new Error('A Guion module did not export its memory');
  return instance.exports;
}

export async function loadModules(compilerBytes, vmBytes) {
  const [compiler, vm] = await Promise.all([instantiate(compilerBytes), instantiate(vmBytes)]);
  if (typeof vm.guion_wasm_abi_version !== 'function' || vm.guion_wasm_abi_version() >>> 16 !== 1) {
    throw new Error('This page needs Guion browser ABI 1.x');
  }
  return { compiler, vm };
}

const readBytes = (exports, pointer, length) => new Uint8Array(exports.memory.buffer, pointer, length).slice();

function place(exports, allocate, data) {
  const pointer = allocate(data.byteLength);
  if (pointer === 0) return 0;
  new Uint8Array(exports.memory.buffer, pointer, data.byteLength).set(data);
  return pointer;
}

/** Turn "CODE (line 2, column 19): message" from the compiler adapter into fields. */
function parseDiagnostic(text) {
  const match = /^([A-Z][A-Z0-9_]+)(?: \(line (\d+), column (\d+)\))?: ([\s\S]*)$/.exec(text);
  if (!match) return { code: 'E_COMPILE', message: text };
  const [, code, line, column, message] = match;
  return { code, message, ...(line ? { line: Number(line), column: Number(column) } : {}) };
}

export function compile(modules, sourceText) {
  const source = encoder.encode(sourceText);
  if (sourceText.trim() === '') return { ok: false, code: 'E_SOURCE_EMPTY', message: 'There is no source to compile.' };
  if (source.byteLength > LIMITS.sourceBytes) {
    return { ok: false, code: 'E_SOURCE_TOO_LARGE', message: `The source is ${source.byteLength} bytes; the playground accepts at most ${LIMITS.sourceBytes}.` };
  }
  const { compiler } = modules;
  const pointer = place(compiler, compiler.allocate_source, source);
  if (pointer === 0) return { ok: false, code: 'E_SOURCE_TOO_LARGE', message: `The playground accepts at most ${LIMITS.sourceBytes} bytes.` };
  let status;
  try {
    status = compiler.compile_source(pointer, source.byteLength);
  } finally {
    compiler.release_source(pointer, source.byteLength);
  }
  if (status !== 0) {
    return { ok: false, ...parseDiagnostic(decoder.decode(readBytes(compiler, compiler.result_error_ptr(), compiler.result_error_len()))) };
  }
  return { ok: true, artifact: readBytes(compiler, compiler.result_artifact_ptr(), compiler.result_artifact_len()) };
}

export function run(modules, artifact, limits = {}) {
  const { vm } = modules;
  const request = encoder.encode(JSON.stringify({
    abi: ABI,
    entry: 'main',
    arguments: [],
    capabilities: [],
    limits: { steps: limits.steps ?? LIMITS.steps, outputBytes: limits.outputBytes ?? LIMITS.outputBytes },
  }));
  const artifactPointer = place(vm, vm.allocate_input, artifact);
  const requestPointer = artifactPointer === 0 ? 0 : place(vm, vm.allocate_input, request);
  try {
    if (artifactPointer === 0 || requestPointer === 0) {
      return { ok: false, code: 'E_ARTIFACT_TOO_LARGE', message: 'The compiled program is too large for the browser VM.', artifactBytes: artifact.byteLength };
    }
    vm.execute_artifact_v1(artifactPointer, artifact.byteLength, requestPointer, request.byteLength);
    // The response is the source of truth; copy it before any further VM call.
    const response = JSON.parse(decoder.decode(readBytes(vm, vm.result_response_ptr(), vm.result_response_len())));
    return {
      ok: response.ok === true,
      stdout: response.stdout ?? '',
      steps: response.steps ?? 0,
      artifactBytes: artifact.byteLength,
      ...(response.ok ? { value: response.value?.value } : { code: response.code, message: response.message }),
    };
  } finally {
    if (artifactPointer !== 0) vm.release_input(artifactPointer, artifact.byteLength);
    if (requestPointer !== 0) vm.release_input(requestPointer, request.byteLength);
  }
}

const lines = (text) => (text === '' ? 0 : text.replace(/\n$/, '').split('\n').length);
const plural = (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`;

/** Plain-words summary of an outcome from compile() or run(). */
export function describe(outcome) {
  if (outcome.ok) {
    return {
      headline: 'It ran.',
      detail: `It printed ${plural(lines(outcome.stdout), 'line')} and finished in ${outcome.steps} of ${LIMITS.steps.toLocaleString('en-US')} steps.`,
    };
  }
  const where = outcome.line ? ` Look at line ${outcome.line}, column ${outcome.column}.` : '';
  switch (outcome.code) {
    case 'R_HOST_UNSUPPORTED':
      return {
        headline: 'The browser said no.',
        detail: 'This program asked the host to do something outside itself, such as reading a file, calling a network, or reading the clock. The playground grants nothing, so the request was refused. In a real project you grant a program exactly what it needs and nothing more.',
      };
    case 'R_STEP_LIMIT':
      return {
        headline: 'It ran out of steps.',
        detail: `The playground stops any program after ${LIMITS.steps.toLocaleString('en-US')} steps, so a loop that never ends cannot freeze your tab.`,
      };
    case 'E_SOURCE_EMPTY':
      return { headline: 'There is nothing to run yet.', detail: 'Type a Guion program, or pick one of the examples.' };
    case 'E_SOURCE_TOO_LARGE':
      return { headline: 'That is too much source for the playground.', detail: outcome.message };
    case 'E_IMPORT_PROJECT_REQUIRED':
      return { headline: 'Imports need a project.', detail: 'The playground compiles a single file and has no files to import from. In a project, imports resolve inside the project root.' };
    case 'E_LOAD':
      return { headline: 'The playground files did not load.', detail: `Check your connection and press Run again. (${outcome.message})` };
    default:
      if (outcome.code?.startsWith('E_')) {
        return { headline: 'Guion would not compile this.', detail: `${outcome.message}.${where}` };
      }
      return { headline: 'The program stopped.', detail: `${outcome.code ?? 'Error'}: ${outcome.message ?? 'no message'}` };
  }
}

// ---- Page wiring: only when the page has the playground controls. -------------------------
function wire() {
  const button = document.querySelector('#pg-run');
  const editor = document.querySelector('#pg-source');
  const output = document.querySelector('#pg-output');
  const status = document.querySelector('#pg-status');
  const meta = document.querySelector('#pg-meta');
  const chooser = document.querySelector('#pg-sample');
  if (!button || !editor || !output || !status) return;

  let loading = null;
  const base = (name) => new URL(name, import.meta.url);
  const fetchBytes = async (name) => {
    const response = await fetch(base(name));
    if (!response.ok) throw new Error(`${name} returned ${response.status}`);
    return new Uint8Array(await response.arrayBuffer());
  };
  const modulesOnce = () => (loading ??= Promise.all([fetchBytes('guion_compiler.wasm'), fetchBytes('guion_vm.wasm')])
    .then(([compilerBytes, vmBytes]) => loadModules(compilerBytes, vmBytes))
    .catch((error) => { loading = null; throw error; }));

  const show = (outcome) => {
    const text = describe(outcome);
    status.textContent = text.headline;
    output.textContent = outcome.ok ? outcome.stdout : text.detail;
    if (outcome.ok && output.textContent === '') output.textContent = '(the program printed nothing)';
    if (meta) {
      meta.textContent = outcome.steps !== undefined
        ? `${outcome.steps} / ${LIMITS.steps.toLocaleString('en-US')} steps · ${outcome.artifactBytes} artifact bytes · ABI 1.0 · 0 host grants`
        : '';
    }
    output.dataset.state = outcome.ok ? 'ok' : 'error';
    if (outcome.ok) status.insertAdjacentText('beforeend', ` ${text.detail}`);
  };

  button.dataset.ready = 'true';
  button.addEventListener('click', async () => {
    button.disabled = true;
    status.textContent = 'Loading the compiler and the VM…';
    try {
      const modules = await modulesOnce();
      const compiled = compile(modules, editor.value);
      show(compiled.ok ? run(modules, compiled.artifact) : compiled);
    } catch (error) {
      show({ ok: false, code: 'E_LOAD', message: String(error.message ?? error) });
    } finally {
      button.disabled = false;
    }
  });

  const loadExample = async (name) => {
    const response = await fetch(base(`samples/${name}.gu`));
    if (!response.ok) throw new Error(`sample returned ${response.status}`);
    editor.value = await response.text();
    if (chooser) chooser.value = name;
    output.textContent = '';
    delete output.dataset.state;
    status.textContent = 'Press Run.';
    if (meta) meta.textContent = '';
  };

  chooser?.addEventListener('change', async () => {
    try {
      await loadExample(chooser.value);
    } catch (error) {
      status.textContent = `Could not load that example. (${error.message})`;
    }
  });

  // Buttons elsewhere on the page ("Open in the playground", "Ask for a file") load an example and scroll up to it.
  for (const trigger of document.querySelectorAll('[data-pg-load]')) {
    trigger.addEventListener('click', async () => {
      try {
        await loadExample(trigger.dataset.pgLoad);
      } catch (error) {
        status.textContent = `Could not load that example. (${error.message})`;
      }
      const calm = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
      document.getElementById('try')?.scrollIntoView({ behavior: calm ? 'auto' : 'smooth' });
      editor.focus({ preventScroll: true });
    });
  }
}

if (typeof document !== 'undefined') wire();
