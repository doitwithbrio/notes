import * as Automerge from '@automerge/automerge';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { StoredNoteDoc } from './document-adapter.js';
import { createAutomergeProsemirrorAdapter } from './automerge-prosemirror-adapter.js';

const tauriApiMock = vi.hoisted(() => ({
  ensureBlobAvailable: vi.fn(async () => ({ available: true, fetched: false })),
  getImage: vi.fn(async () => new Uint8Array([1, 2, 3])),
  importImage: vi.fn(async () => ({ hash: 'imported-hash' })),
}));

vi.mock('../api/tauri.js', () => ({
  tauriApi: tauriApiMock,
}));

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

async function flushMicrotasks() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
}

async function waitFor(check: () => void, attempts = 20) {
  let lastError: unknown;
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try {
      check();
      return;
    } catch (error) {
      lastError = error;
      await flushMicrotasks();
    }
  }
  throw lastError;
}

function makeDoc(content: NonNullable<StoredNoteDoc['doc']>['content']) {
  return Automerge.from<StoredNoteDoc>({
    schemaVersion: 2,
    doc: {
      type: 'doc',
      content,
    },
    text: '',
  });
}

function makeImageFile(name = 'diagram.png') {
  const bytes = Uint8Array.from([1, 2, 3, 4]);
  const file = new File([bytes], name, { type: 'image/png' });
  Object.defineProperty(file, 'arrayBuffer', {
    value: async () => bytes.buffer.slice(0),
  });
  return { file, bytes };
}

describe('automerge prosemirror adapter blob images', () => {
  const originalCreateObjectUrl = URL.createObjectURL;

  beforeEach(() => {
    document.body.innerHTML = '';
    tauriApiMock.ensureBlobAvailable.mockReset();
    tauriApiMock.getImage.mockReset();
    tauriApiMock.importImage.mockReset();
    tauriApiMock.ensureBlobAvailable.mockImplementation(async () => ({ available: true, fetched: false }));
    tauriApiMock.getImage.mockImplementation(async () => new Uint8Array([1, 2, 3]));
    tauriApiMock.importImage.mockImplementation(async () => ({ hash: 'imported-hash' }));
    URL.createObjectURL = vi.fn(() => 'blob:mock-url');
  });

  afterEach(() => {
    document.body.innerHTML = '';
    URL.createObjectURL = originalCreateObjectUrl;
  });

  it('lazy fetches blob images and shows loading before ready', async () => {
    const host = document.createElement('div');
    document.body.appendChild(host);
    const gate = deferred<{ available: boolean; fetched: boolean }>();
    tauriApiMock.ensureBlobAvailable.mockImplementationOnce(async () => gate.promise);
    tauriApiMock.getImage.mockImplementationOnce(async () => new Uint8Array([9, 8, 7]));

    const adapter = createAutomergeProsemirrorAdapter(host, {
      getProjectId: () => 'project-1',
    });
    adapter.attach(makeDoc([{
      type: 'paragraph',
      content: [{ type: 'image', attrs: { src: 'blob:image-hash', alt: 'diagram' } }],
    }]), true);

    const node = host.querySelector('.blob-image-node') as HTMLSpanElement;
    expect(node.dataset.state).toBe('loading');
    expect(node.textContent).toContain('loading image...');
    expect(tauriApiMock.ensureBlobAvailable).toHaveBeenCalledWith('project-1', 'image-hash');
    expect(tauriApiMock.getImage).not.toHaveBeenCalled();

    gate.resolve({ available: true, fetched: true });

    await waitFor(() => {
      expect(node.dataset.state).toBe('ready');
      expect((node.querySelector('img') as HTMLImageElement).src).toContain('blob:mock-url');
    });
    expect(tauriApiMock.getImage).toHaveBeenCalledWith('image-hash');
    const ensureOrder = tauriApiMock.ensureBlobAvailable.mock.invocationCallOrder[0];
    const imageOrder = tauriApiMock.getImage.mock.invocationCallOrder[0];
    expect(ensureOrder).toBeDefined();
    expect(imageOrder).toBeDefined();
    expect(ensureOrder!).toBeLessThan(imageOrder!);

    adapter.detach();
  });

  it('shows a missing placeholder when the blob is unavailable', async () => {
    const host = document.createElement('div');
    document.body.appendChild(host);
    tauriApiMock.ensureBlobAvailable.mockImplementationOnce(async () => ({
      available: false,
      fetched: false,
    }));

    const adapter = createAutomergeProsemirrorAdapter(host, {
      getProjectId: () => 'project-1',
    });
    adapter.attach(makeDoc([{
      type: 'paragraph',
      content: [{ type: 'image', attrs: { src: 'blob:missing-hash', alt: 'missing' } }],
    }]), true);

    const node = host.querySelector('.blob-image-node') as HTMLSpanElement;
    await waitFor(() => {
      expect(node.dataset.state).toBe('missing');
      expect(node.textContent).toContain('image unavailable');
    });
    expect(tauriApiMock.getImage).not.toHaveBeenCalled();

    adapter.detach();
  });

  it('imports pasted images as blob-backed image nodes', async () => {
    const host = document.createElement('div');
    document.body.appendChild(host);
    tauriApiMock.importImage.mockResolvedValueOnce({ hash: 'paste-hash' });

    const adapter = createAutomergeProsemirrorAdapter(host, {
      getProjectId: () => 'project-1',
    });
    adapter.attach(makeDoc([{
      type: 'paragraph',
      content: [],
    }]), true);
    const editor = adapter.getEditor();
    expect(editor).toBeTruthy();

    const { file } = makeImageFile('paste.png');
    const event = new Event('paste', { bubbles: true, cancelable: true }) as ClipboardEvent;
    Object.defineProperty(event, 'clipboardData', {
      value: {
        files: [file],
        getData: () => '',
      },
    });

    editor!.view.dom.dispatchEvent(event);

    await waitFor(() => {
      expect(tauriApiMock.importImage).toHaveBeenCalledWith(
        'project-1',
        expect.any(Uint8Array),
        'paste.png',
      );
      expect(JSON.stringify(editor!.getJSON())).toContain('blob:paste-hash');
    });

    adapter.detach();
  });

  it('imports dropped images as blob-backed image nodes', async () => {
    const host = document.createElement('div');
    document.body.appendChild(host);
    tauriApiMock.importImage.mockResolvedValueOnce({ hash: 'drop-hash' });

    const adapter = createAutomergeProsemirrorAdapter(host, {
      getProjectId: () => 'project-1',
    });
    adapter.attach(makeDoc([{
      type: 'paragraph',
      content: [],
    }]), true);
    const editor = adapter.getEditor();
    expect(editor).toBeTruthy();
    vi.spyOn(editor!.view, 'posAtCoords').mockReturnValue({ pos: 1, inside: 0 });

    const { file } = makeImageFile('drop.png');
    const event = new Event('drop', { bubbles: true, cancelable: true }) as DragEvent;
    Object.defineProperty(event, 'dataTransfer', {
      value: {
        files: [file],
        getData: () => '',
      },
    });
    Object.defineProperty(event, 'clientX', { value: 12 });
    Object.defineProperty(event, 'clientY', { value: 16 });

    editor!.view.dom.dispatchEvent(event);

    await waitFor(() => {
      expect(tauriApiMock.importImage).toHaveBeenCalledWith(
        'project-1',
        expect.any(Uint8Array),
        'drop.png',
      );
      expect(JSON.stringify(editor!.getJSON())).toContain('blob:drop-hash');
    });

    adapter.detach();
  });
});
