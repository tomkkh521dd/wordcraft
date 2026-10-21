import native from 'libwordcraft.so';
import { display, font, window } from '@kit.ArkUI';
import { displaySync } from '@kit.ArkGraphics2D';
import { common, ConfigurationConstant } from '@kit.AbilityKit';
import { i18n } from '@kit.LocalizationKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { fileIo, fileUri, picker } from '@kit.CoreFileKit';
import { image } from '@kit.ImageKit';
import { pasteboard } from '@kit.BasicServicesKit';

const DOMAIN: number = 0x0000;
const TAG: string = 'WordCraft';

/** Pointer kinds, buttons and modifier bits, as `wc_abi.h` numbers them. */
const POINTER_DOWN: number = 0;
const POINTER_UP: number = 1;
const POINTER_MOVE: number = 2;
const POINTER_LEAVE: number = 3;
const POINTER_CANCEL: number = 4;
const BUTTON_PRIMARY: number = 0;
const BUTTON_SECONDARY: number = 1;
const BUTTON_MIDDLE: number = 2;
const BUTTON_NONE: number = 3;
const MOD_SHIFT: number = 1;
const MOD_CTRL: number = 2;
const MOD_ALT: number = 4;
const MOD_META: number = 8;
const MOD_CAPS: number = 16;
/** Most pixels of a clipboard picture that may be pasted (`paste_picture::MAX_PIXELS`). */
const MAX_PASTE_PIXELS: number = 50000000;
/** How often the working copies are compared with the files the user picked (ms). */
const COPY_OUT_INTERVAL_MS: number = 500;

/** The display's density (px per vp); every size the surface reports is px. */
function density(): number {
  try {
    return display.getDefaultDisplaySync().densityPixels;
  } catch (err) {
    hilog.warn(DOMAIN, TAG, 'no display: %{public}s', JSON.stringify(err));
    return 1.0;
  }
}

/**
 * The XComponent's surface lifecycle. ArkUI gives the surface an id; the C++ side turns that into
 * a native window whose buffer queue it writes frames into (no GPU context).
 */
class WordcraftController extends XComponentController {
  onSurfaceCreated(surfaceId: string): void {
    const rect = this.getXComponentSurfaceRect();
    const id = BigInt(surfaceId);
    hilog.info(DOMAIN, TAG, 'surface %{public}s created, %{public}dx%{public}d', surfaceId, rect.surfaceWidth, rect.surfaceHeight);
    native.surface(id, rect.surfaceWidth, rect.surfaceHeight, density());
  }

  onSurfaceChanged(surfaceId: string, rect: SurfaceRect): void {
    hilog.info(DOMAIN, TAG, 'surface %{public}s now %{public}dx%{public}d', surfaceId, rect.surfaceWidth, rect.surfaceHeight);
    native.resize(rect.surfaceWidth, rect.surfaceHeight, density());
  }

  onSurfaceDestroyed(surfaceId: string): void {
    hilog.info(DOMAIN, TAG, 'surface %{public}s destroyed', surfaceId);
    native.surfaceGone();
  }
}

/** The modifier bits the app understands, from a key or mouse event. */
function modifierBits(event: KeyEvent | MouseEvent): number {
  let bits: number = 0;
  const has = (key: string): boolean => {
    try {
      return event.getModifierKeyState !== undefined && event.getModifierKeyState([key]) === true;
    } catch (err) {
      return false;
    }
  };
  if (has('Shift')) {
    bits |= MOD_SHIFT;
  }
  if (has('Ctrl')) {
    bits |= MOD_CTRL;
  }
  if (has('Alt')) {
    bits |= MOD_ALT;
  }
  if (has('Meta')) {
    bits |= MOD_META;
  }
  if (has('CapsLock')) {
    bits |= MOD_CAPS;
  }
  return bits;
}

function mouseKind(action: MouseAction): number {
  switch (action) {
    case MouseAction.Press:
      return POINTER_DOWN;
    case MouseAction.Release:
      return POINTER_UP;
    case MouseAction.CANCEL:
      return POINTER_CANCEL;
    case MouseAction.LEAVE_WINDOW:
      return POINTER_LEAVE;
    default:
      return POINTER_MOVE;
  }
}

function mouseButton(button: MouseButton): number {
  switch (button) {
    case MouseButton.Left:
      return BUTTON_PRIMARY;
    case MouseButton.Right:
      return BUTTON_SECONDARY;
    case MouseButton.Middle:
      return BUTTON_MIDDLE;
    default:
      return BUTTON_NONE;
  }
}

/** A request from the Rust side (see crates/ohos-app/src/host.rs). */
interface HostRequest {
  id: number;
  kind: string;
  purpose?: string;
  many?: boolean;
  name?: string;
  html?: string;
  text?: string;
  mathml?: string;
}

/** What a file dialog may pick, by the purpose the UI gave it. */
function suffixFilters(purpose: string): string[] {
  switch (purpose) {
    case 'picture':
      return ['图片|.png,.jpg,.jpeg,.gif,.webp,.bmp,.svg'];
    case 'font':
      return ['字体|.ttf,.otf,.ttc,.woff,.woff2'];
    case 'recipients':
      return ['收件人列表|.csv,.txt'];
    case 'sources':
      return ['文献库|.xml,.ris,.bib,.json'];
    default:
      return ['文档|.docx,.docm,.dotx,.dotm,.doc,.dot,.odt,.rtf,.txt,.md,.html,.htm,.pdf'];
  }
}

@Entry
@Component
struct Index {
  private controller: WordcraftController = new WordcraftController();
  private sync: displaySync.DisplaySync | undefined = undefined;
  private presented: number = 0;
  private started: boolean = false;
  /**
   * Where each working copy in the app's sandbox belongs in the user's folders. The picker grants
   * a file, not its folder, so the engine can't save beside the user's file (a save needs a
   * temporary file beside its destination); it edits a sandbox copy instead, and
   * `copyWorkingCopies` writes the finished file back to the picked URI.
   */
  private targets: Map<string, string> = new Map<string, string>();
  /** What each working copy looked like when it was last written back (`size:mtime`). */
  private synced: Map<string, string> = new Map<string, string>();
  private copyTimer: number = -1;

  aboutToAppear(): void {
    const context: common.UIAbilityContext | undefined = this.getUIContext().getHostContext() as common.UIAbilityContext;
    const settingsDir: string = context !== undefined ? `${context.filesDir}/settings` : '';
    const tmpDir: string = context !== undefined ? context.tempDir : '';

    // The system's font folders differ per device, so the app gets them at run time.
    let fontDirs: string[] = [];
    try {
      fontDirs = font.getUIFontConfig().fontDir;
    } catch (err) {
      hilog.warn(DOMAIN, TAG, 'no system font folders: %{public}s', JSON.stringify(err));
    }

    // "zh-Hans-CN" style: what LANG takes and sys-locale reads.
    let locale: string = 'en-US';
    try {
      locale = i18n.System.getSystemLocaleInstance().toString();
    } catch (err) {
      hilog.warn(DOMAIN, TAG, 'no system locale: %{public}s', JSON.stringify(err));
      try {
        locale = i18n.System.getSystemLanguage();
      } catch (errToo) {
        hilog.warn(DOMAIN, TAG, 'no system language either: %{public}s', JSON.stringify(errToo));
      }
    }

    let theme: string = 'auto';
    const mode: ConfigurationConstant.ColorMode | undefined = context !== undefined ? context.config.colorMode : undefined;
    if (mode === ConfigurationConstant.ColorMode.COLOR_MODE_DARK) {
      theme = 'dark';
    } else if (mode === ConfigurationConstant.ColorMode.COLOR_MODE_LIGHT) {
      theme = 'light';
    }

    const rc: number = native.init(settingsDir, tmpDir, fontDirs, locale, theme, 'sample');
    hilog.info(DOMAIN, TAG, 'app started (%{public}d) with %{public}d font folder(s)', rc, fontDirs.length);
    this.started = rc === 0;
    this.startFrames();
    // Save, export and Print-to-PDF all write into the sandbox; this puts the result where the
    // user picked, half a second later at the latest.
    this.copyTimer = setInterval(() => this.copyWorkingCopies(), COPY_OUT_INTERVAL_MS);
  }

  aboutToDisappear(): void {
    this.stopFrames();
    if (this.copyTimer !== -1) {
      clearInterval(this.copyTimer);
      this.copyTimer = -1;
    }
    native.destroy();
    hilog.info(DOMAIN, TAG, 'app stopped after %{public}d frame(s)', this.presented);
  }

  /** One frame per display refresh; the app decides whether there is anything to draw. */
  private startFrames(): void {
    if (!this.started) {
      return;
    }
    this.sync = displaySync.create();
    this.sync.setExpectedFrameRateRange({ expected: 60, min: 30, max: 120 });
    this.sync.on('frame', () => {
      // The Rust side asks for a picker or the clipboard here; one request per frame.
      const request = native.hostTake();
      if (request !== null) {
        this.answerHost(request);
      }
      if (native.render() === 1) {
        this.presented++;
        if (this.presented % 300 === 0) {
          hilog.info(DOMAIN, TAG, 'stats %{public}s', native.stats());
        }
      }
    });
    this.sync.start();
  }

  /** Run one request from the Rust side and answer it (the answer may take a while: a picker). */
  private answerHost(request: string): void {
    let parsed: HostRequest;
    try {
      parsed = JSON.parse(request) as HostRequest;
    } catch (err) {
      hilog.error(DOMAIN, TAG, 'host request is not JSON: %{public}s', JSON.stringify(err));
      return;
    }
    const answer = (json: string): void => native.hostAnswer(parsed.id, json);
    const failed = (err: Error): void => {
      // The interesting one on the device: reading the clipboard needs a grant (see
      // docs/platform-ohos.md), and a picker can be unavailable.
      hilog.error(DOMAIN, TAG, 'host request %{public}s failed: %{public}s', parsed.kind, JSON.stringify(err));
      answer(JSON.stringify({ error: `${err}` }));
    };
    if (parsed.kind === 'pickOpen') {
      this.pickOpen(parsed).then((json: string) => answer(json)).catch(failed);
    } else if (parsed.kind === 'pickSave') {
      this.pickSave(parsed).then((json: string) => answer(json)).catch(failed);
    } else if (parsed.kind === 'clipboardRead') {
      this.readClipboard().then((json: string) => answer(json)).catch(failed);
    } else if (parsed.kind === 'clipboardWrite') {
      this.writeClipboard(parsed).then((json: string) => answer(json)).catch(failed);
    } else {
      answer(JSON.stringify({ error: `unknown request ${parsed.kind}` }));
    }
  }

  /**
   * Choose documents. Each pick becomes a working copy in the app's sandbox: the picker grants a
   * file, not its folder, so the engine (which saves through a temporary file beside its
   * destination) can't write beside the user's file. `copyWorkingCopies` writes the edits back.
   */
  private async pickOpen(request: HostRequest): Promise<string> {
    const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
    const options = new picker.DocumentSelectOptions();
    options.maxSelectNumber = request.many === true ? 20 : 1;
    options.fileSuffixFilters = suffixFilters(request.purpose ?? 'document');
    const uris: string[] = await new picker.DocumentViewPicker(context).select(options);
    if (uris.length === 0) {
      return JSON.stringify({ cancelled: true });
    }
    const paths: string[] = [];
    for (const uri of uris) {
      const copy: string = this.workingCopy(uri);
      if (copy !== '') {
        paths.push(copy);
      }
    }
    hilog.info(DOMAIN, TAG, 'picked %{public}d of %{public}d file(s)', paths.length, uris.length);
    if (paths.length === 0) {
      return JSON.stringify({ error: 'the picked file could not be read' });
    }
    if (paths.length === 1) {
      return JSON.stringify({ path: paths[0] });
    }
    return JSON.stringify({ paths: paths });
  }

  /**
   * Choose where to save. The answer is a fresh working copy in the sandbox — the engine writes
   * there, and `copyWorkingCopies` puts the file where the user pointed.
   */
  private async pickSave(request: HostRequest): Promise<string> {
    const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
    const options = new picker.DocumentSaveOptions();
    const name: string = request.name ?? 'Document.docx';
    options.newFileNames = [name];
    options.fileSuffixChoices = ['Word 文档|.docx', 'PDF|.pdf', '纯文本|.txt', '所有文件|.*'];
    const uris: string[] = await new picker.DocumentViewPicker(context).save(options);
    if (uris.length === 0) {
      return JSON.stringify({ cancelled: true });
    }
    const uri: string = uris[0];
    const copy: string = this.workingPath(name);
    if (copy === '') {
      return JSON.stringify({ error: 'the app has nowhere to keep the document' });
    }
    this.targets.set(copy, uri);
    this.synced.delete(copy);
    hilog.info(DOMAIN, TAG, 'saving through %{public}s to the picked file', copy);
    return JSON.stringify({ path: copy });
  }

  /** The sandbox folder the working copies live in, created on first use ('' when it can't be). */
  private documentsDir(): string {
    const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
    const dir: string = `${context.filesDir}/documents`;
    try {
      fileIo.mkdirSync(dir);
    } catch (err) {
      // Already there (the usual case) — any other failure shows up when a copy is opened.
    }
    return dir;
  }

  /** A working copy of the picked document, and the URI its edits belong to ('' when it fails). */
  private workingCopy(uri: string): string {
    const path: string = this.filePath(uri);
    const name: string = path !== '' ? path.substring(path.lastIndexOf('/') + 1) : `document-${Date.now()}`;
    const copy: string = this.workingPath(name);
    if (copy === '') {
      return '';
    }
    try {
      const source: fileIo.File = fileIo.openSync(uri, fileIo.OpenMode.READ_ONLY);
      try {
        fileIo.copyFileSync(source.fd, copy, 0);
      } finally {
        fileIo.closeSync(source);
      }
    } catch (err) {
      hilog.error(DOMAIN, TAG, 'could not read the picked file: %{public}s', JSON.stringify(err));
      return '';
    }
    this.targets.set(copy, uri);
    this.markSynced(copy);
    return copy;
  }

  /**
   * An unused sandbox path for `name`. Each working copy gets its own numbered folder, so the file
   * keeps exactly the name the user's file has (the app's title bar shows it).
   */
  private workingPath(name: string): string {
    const dir: string = this.documentsDir();
    const base: string = name.replace(/[\/:*?"<>|]/g, '_').replace(/^\.+/, '_');
    let n: number = 1;
    let candidate: string = `${dir}/${n}/${base}`;
    while (this.targets.has(candidate)) {
      n++;
      candidate = `${dir}/${n}/${base}`;
    }
    try {
      fileIo.mkdirSync(`${dir}/${n}`);
    } catch (err) {
      // Already there, or the copy below reports what is wrong.
    }
    return candidate;
  }

  /** Remember what the working copy looks like now, so an unchanged one isn't copied again. */
  private markSynced(copy: string): void {
    try {
      const info: fileIo.Stat = fileIo.statSync(copy);
      this.synced.set(copy, `${info.size}:${info.mtime}`);
    } catch (err) {
      this.synced.delete(copy);
    }
  }

  /** Write every working copy whose file changed (a save, an export, a PDF) back to its URI. */
  private copyWorkingCopies(): void {
    this.targets.forEach((uri: string, copy: string) => {
      let marker: string = '';
      try {
        const info: fileIo.Stat = fileIo.statSync(copy);
        marker = `${info.size}:${info.mtime}`;
      } catch (err) {
        return; // not written yet
      }
      if (this.synced.get(copy) === marker) {
        return;
      }
      try {
        const source: fileIo.File = fileIo.openSync(copy, fileIo.OpenMode.READ_ONLY);
        try {
          const destination: fileIo.File = fileIo.openSync(uri, fileIo.OpenMode.WRITE_ONLY | fileIo.OpenMode.TRUNC);
          try {
            fileIo.copyFileSync(source.fd, destination.fd, 0);
          } finally {
            fileIo.closeSync(destination);
          }
        } finally {
          fileIo.closeSync(source);
        }
        this.synced.set(copy, marker);
        hilog.info(DOMAIN, TAG, 'wrote the document back to the picked file (%{public}s bytes)', marker);
      } catch (err) {
        hilog.error(DOMAIN, TAG, 'could not write the document back: %{public}s', JSON.stringify(err));
      }
    });
  }

    /** A `file://docs/…` URI as the sandbox path the engine works with ('' when it is not a file). */
  private filePath(uri: string): string {
    try {
      return new fileUri.FileUri(uri).path;
    } catch (err) {
      hilog.warn(DOMAIN, TAG, 'not a file URI: %{public}s', JSON.stringify(err));
      return '';
    }
  }

  /**
   * The system clipboard: its text, its HTML, a picture (packed to a file) and copied file URIs,
   * plus the MIME types, so a paste that finds nothing to use can say what is there.
   */
  private async readClipboard(): Promise<string> {
    const data = await pasteboard.getSystemPasteboard().getData();
    let text: string = '';
    let html: string = '';
    let picture: string = '';
    const formats: string[] = [];
    const uris: string[] = [];
    const count: number = data.getRecordCount();
    for (let i = 0; i < count; i++) {
      const record: pasteboard.PasteDataRecord = data.getRecord(i);
      const mime: string = record.mimeType;
      if (!formats.includes(mime)) {
        formats.push(mime);
      }
      if (mime === pasteboard.MIMETYPE_TEXT_PLAIN && text === '') {
        text = record.toPlainText();
      } else if (mime === pasteboard.MIMETYPE_TEXT_HTML && html === '') {
        html = record.toPlainText();
      } else if (mime === pasteboard.MIMETYPE_PIXELMAP && picture === '') {
        picture = await this.packPicture(record);
      } else if (mime === pasteboard.MIMETYPE_TEXT_URI) {
        const path: string = this.filePath(record.toPlainText());
        if (path !== '') {
          uris.push(path);
        }
      }
    }
    hilog.info(DOMAIN, TAG, 'clipboard read: %{public}d record(s), %{public}d text chars, %{public}s', count, text.length, formats.join(','));
    return JSON.stringify({ text: text, html: html, picture: picture, uris: uris, formats: formats });
  }

  /**
   * A clipboard `PixelMap` as a PNG in the app's cache folder; the Rust side reads the bytes and
   * removes the file. Returns '' when the picture is too large or cannot be packed.
   */
  private async packPicture(record: pasteboard.PasteDataRecord): Promise<string> {
    const pixelMap: image.PixelMap | undefined = record.pixelMap;
    if (pixelMap === undefined) {
      return '';
    }
    const packer: image.ImagePacker = image.createImagePacker();
    let path: string = '';
    try {
      const info: image.ImageInfo = pixelMap.getImageInfoSync();
      const pixels: number = info.size.width * info.size.height;
      if (pixels <= 0 || pixels > MAX_PASTE_PIXELS) {
        hilog.warn(DOMAIN, TAG, 'clipboard picture is %{public}d pixels: too large to paste', pixels);
        return '';
      }
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      path = `${context.cacheDir}/clipboard-${Date.now()}.png`;
      const file: fileIo.File = fileIo.openSync(path, fileIo.OpenMode.CREATE | fileIo.OpenMode.READ_WRITE | fileIo.OpenMode.TRUNC);
      try {
        // Encoding straight into the file keeps a screenshot out of a second buffer.
        await packer.packToFile(pixelMap, file.fd, { format: 'image/png', quality: 100 });
      } finally {
        fileIo.closeSync(file);
      }
      hilog.info(DOMAIN, TAG, 'clipboard picture packed (%{public}dx%{public}d)', info.size.width, info.size.height);
      return path;
    } catch (err) {
      hilog.warn(DOMAIN, TAG, 'no clipboard picture: %{public}s', JSON.stringify(err));
      if (path !== '') {
        try {
          fileIo.unlinkSync(path);
        } catch (errToo) {
          hilog.warn(DOMAIN, TAG, 'could not remove %{public}s', path);
        }
      }
      return '';
    } finally {
      await packer.release();
    }
  }

  /** Put the plain text, and the HTML alternative with it, on the system clipboard. */
  private async writeClipboard(request: HostRequest): Promise<string> {
    const text: string = request.text ?? '';
    const html: string = request.html ?? '';
    if (text === '' && html === '') {
      return JSON.stringify({ error: 'nothing to copy' });
    }
    // A record's value must not be empty, so an HTML-only copy starts from the HTML record.
    const first: string = text !== '' ? pasteboard.MIMETYPE_TEXT_PLAIN : pasteboard.MIMETYPE_TEXT_HTML;
    const data: pasteboard.PasteData = pasteboard.createData(first, text !== '' ? text : html);
    if (text !== '' && html !== '') {
      data.addRecord(pasteboard.MIMETYPE_TEXT_HTML, html);
    }
    await pasteboard.getSystemPasteboard().setData(data);
    hilog.info(DOMAIN, TAG, 'clipboard written (%{public}d text, %{public}d html chars)', text.length, html.length);
    return JSON.stringify({ ok: true });
  }

  private stopFrames(): void {
    if (this.sync !== undefined) {
      this.sync.stop();
      this.sync = undefined;
    }
  }

  build() {
    Stack() {
      XComponent({ type: XComponentType.SURFACE, controller: this.controller })
        .id('wordcraft')
        .width('100%')
        .height('100%')
        .focusable(true)
        .focusOnTouch(true)
        .defaultFocus(true)
        .onFocus(() => native.focus(true))
        .onBlur(() => native.focus(false))
        .onTouch((event: TouchEvent) => {
          // `touches` is empty on Up/Cancel in some versions; the changed touch is always there.
          const touch: TouchObject | undefined =
            event.touches.length > 0 ? event.touches[0] : (event.changedTouches.length > 0 ? event.changedTouches[0] : undefined);
          if (touch === undefined) {
            return;
          }
          let kind: number = POINTER_MOVE;
          if (event.type === TouchType.Down) {
            kind = POINTER_DOWN;
          } else if (event.type === TouchType.Up) {
            kind = POINTER_UP;
          } else if (event.type === TouchType.Cancel) {
            kind = POINTER_CANCEL;
          }
          native.pointer(kind, touch.x, touch.y, BUTTON_NONE, 0);
        })
        .onMouse((event: MouseEvent) => {
          native.pointer(mouseKind(event.action), event.x, event.y, mouseButton(event.button), modifierBits(event));
        })
        .onAxisEvent((event: AxisEvent) => {
          // A trackpad or wheel: ArkUI's axis values are vp, positive down and right.
          native.scroll(event.getHorizontalAxisValue(), event.getVerticalAxisValue());
        })
        .onKeyEvent((event: KeyEvent) => {
          if (event.type !== KeyType.Down && event.type !== KeyType.Up) {
            return;
          }
          native.key(event.keyCode, event.type === KeyType.Down, false, modifierBits(event), event.keyText);
        })
    }
    .width('100%')
    .height('100%')
  }
}
