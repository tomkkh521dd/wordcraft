import native from 'libwordcraft.so';
import { display, font, window } from '@kit.ArkUI';
import { displaySync } from '@kit.ArkGraphics2D';
import { common, ConfigurationConstant } from '@kit.AbilityKit';
import { inputMethod } from '@kit.IMEKit';
import { i18n } from '@kit.LocalizationKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { fileIo, fileShare, fileUri, picker } from '@kit.CoreFileKit';
import { systemShare } from '@kit.ShareKit';
import { uniformTypeDescriptor as utd } from '@kit.ArkData';
import { image } from '@kit.ImageKit';
import { pasteboard } from '@kit.BasicServicesKit';
import { preferences } from '@kit.ArkData';

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
/** ArkUI key codes the input method's actions turn into (see `@ohos.multimodalInput.keyCode`). */
const KEY_ESCAPE: number = 2070;
const KEY_ENTER: number = 2054;
const KEY_BACKSPACE: number = 2055;
const KEY_DELETE: number = 2071;
const KEY_ARROW_UP: number = 2012;
const KEY_ARROW_DOWN: number = 2013;
const KEY_ARROW_LEFT: number = 2014;
const KEY_ARROW_RIGHT: number = 2015;
/** Picture extensions a copied file may have (lower case), for reading it through its URI. */
const PICTURE_SUFFIXES: string[] = ['.png', '.jpg', '.jpeg', '.gif', '.webp', '.bmp'];
/** The invisible character the composing field holds, so a Backspace has something to delete. */
const IME_FIELD_SENTINEL: string = '​';
/** Whether the keyboard types into a field on the caret (inline composition) or straight at the app.
 *
 * Off: with the Huawei IME on HarmonyOS 6.1.0 a field reset after every change cancels the
 * composition, so 拼音 and 五笔 keys never commit (measured on the tablet: nothing appears).
 * `inputMethod.attach` commits correctly; the field needs `onChange`'s `previewText` parameter to
 * tell composition from committed text before it can come back. */
const USE_IME_FIELD: boolean = false;
/** The selection handle's finger-sized hit area (vp). */
const HANDLE_TOUCH: number = 44;
/** How close to the page's top or bottom a dragged handle scrolls, and by how much (vp). */
const EDGE_SCROLL: number = 40;
const EDGE_SCROLL_STEP: number = 24;
/** How long a still finger takes to open the context menu (ms). */
const LONG_PRESS_MS: number = 600;
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

/** Two numbers in vp (the fingers' midpoint), kept out of ArkUI's `Position` (Length, optional). */
interface FingerMid {
  x: number;
  y: number;
}

/** A request from the Rust side (see crates/ohos-app/src/host.rs). */
interface HostRequest {
  id: number;
  kind: string;
  path?: string;
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
  /**
   * The platform input method, attached to the self-drawn document while it has a caret: the
   * canvas is not a text component, so nothing else would bring up the soft keyboard.
   */
  private ime: inputMethod.InputMethodController = inputMethod.getController();
  private imeAttached: boolean = false;
  /**
   * The composing field: a one-line `TextInput` sitting exactly on the caret, styled like page text
   * and kept empty. The platform's IME targets it, so 拼音 appears **in the page** (underlined, with
   * the candidate bar above it) instead of only in the keyboard's own bubble — which is what a
   * self-drawn client gets from `inputMethod.attach` alone on this firmware. Committed text arrives
   * through `onChange` and goes into the document.
   */
  @State private imeFieldX: number = 0;
  @State private imeFieldY: number = 0;
  @State private imeFieldWidth: number = 0;
  @State private imeFieldHeight: number = 0;
  @State private imeFieldValue: string = IME_FIELD_SENTINEL;
  @State private imeFieldShown: boolean = false;
  private imeField: TextInputController = new TextInputController();
  /** Whether the interface is in the dark theme (the composing field matches the page's text). */
  private themeDark: boolean = false;
  /** Fingers on the page: two or more means a pinch or a scroll, never a selection drag. */
  private touching: number = 0;
  /** Set while a multi-finger gesture owns the page, so single-finger events are ignored. */
  private multiTouch: boolean = false;
  /** The finger spread the pinch started from (vp), and the scale already sent to the page. */
  private pinchStart: number = 0;
  private pinchApplied: number = 1;
  /** The two fingers' midpoint, its velocity (vp/s) and when it last moved. */
  private panMidX: number = 0;
  private panMidY: number = 0;
  private panVelocityX: number = 0;
  private panVelocityY: number = 0;
  private panMovedAt: number = 0;
  /**
   * Set when a long press opened the context menu: the finger's own up event must not reach the
   * page, or it would close the menu the press just opened. Cleared when every finger is up.
   */
  private afterLongPress: boolean = false;
  /**
   * Where the current single-finger press started and whether it moved far enough to be a drag.
   * A long press that turned into a drag must not open the context menu: dragging selects text.
   */
  private pressX: number = 0;
  private pressY: number = 0;
  private pressDragged: boolean = false;
  /** The long-press timer of the current single-finger press (-1: none). */
  private pressTimer: number = -1;
  /**
   * The selection handle: a draggable dot at the caret (the end of the selection), so a selection
   * can be adjusted with a finger instead of re-dragging from the start. Its position is read from
   * the page every frame, so it follows the caret wherever typing, scrolling or a menu moves it.
   */
  @State private handleVisible: boolean = false;
  @State private handleIsRange: boolean = false;
  @State private handleX: number = 0;
  @State private handleY: number = 0;
  @State private handleHeight: number = 0;
  /** The document the app had open, so a relaunch reopens it ("resume where you left"). */
  private lastDocument: string = '';
  /** The URI another app asked us to open (the file manager's "open with"). */
  private launchUri: string = '';
  /** Where the sandbox ↔ URI table and the last document are kept between launches. */
  private store: preferences.Preferences | undefined = undefined;

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

    this.themeDark = theme === 'dark';
    const rc: number = native.init(settingsDir, tmpDir, fontDirs, locale, theme, 'sample');
    hilog.info(DOMAIN, TAG, 'app started (%{public}d) with %{public}d font folder(s)', rc, fontDirs.length);
    this.started = rc === 0;
    // The document another app asked us to open, and what was open last time.
    const launch: string | undefined = AppStorage.get<string>('wordcraft.launchUri');
    if (launch !== undefined && launch !== '') {
      this.launchUri = launch;
    }
    // `loadState` tells the app when it is ready: it has to know the last document first.
    this.loadState(context);
    this.startFrames();
    // Save, export and Print-to-PDF all write into the sandbox; this puts the result where the
    // user picked, half a second later at the latest.
    this.copyTimer = setInterval(() => this.copyWorkingCopies(), COPY_OUT_INTERVAL_MS);
  }

  /**
   * The page is in front again: cache the clipboard now (a picture copied in another app is only
   * readable through the copying app's temporary grant, which lapses while we are in the background).
   */
  onPageShow(): void {
    if (this.started) {
      native.clipboardPreload();
    }
  }

  /**
   * HarmonyOS's Back gesture: the first one closes the keyboard (as on every phone and tablet);
   * with no keyboard up the system leaves the app, which is what Back means there.
   */
  onBackPress(): boolean {
    if (this.imeAttached) {
      this.detachInputMethod();
      return true;
    }
    if (native.overlayOpen() === 1) {
      // Close the menu the app drew instead of leaving the app.
      native.key(KEY_ESCAPE, true, false, 0, '');
      native.key(KEY_ESCAPE, false, false, 0, '');
      return true;
    }
    return false;
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
      this.followCaretHandle();
      this.rememberDocument();
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
    } else if (parsed.kind === 'share') {
      this.shareDocument(parsed).then((json: string) => answer(json)).catch(failed);
    } else if (parsed.kind === 'startupDocument') {
      this.startupDocument().then((json: string) => answer(json)).catch(failed);
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
    // The name the user typed in the dialog, not the suggested one: it is what the app shows.
    const picked: string = this.filePath(uri);
    const base: string = picked !== '' ? picked.substring(picked.lastIndexOf('/') + 1) : name;
    const copy: string = this.workingPath(base !== '' ? base : name);
    if (copy === '') {
      return JSON.stringify({ error: 'the app has nowhere to keep the document' });
    }
    this.targets.set(copy, uri);
    this.synced.delete(copy);
    this.persistGrant(uri);
    this.saveTargets();
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
    this.persistGrant(uri);
    this.saveTargets();
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
      const marker: string = this.markerOf(copy);
      if (marker === '' || this.synced.get(copy) === marker) {
        return; // not written yet, or already in the user's file
      }
      if (this.copyOut(copy, uri, marker)) {
        this.saveSynced();
      }
    });
  }

  /**
   * Put one working copy into the user's file and check it arrived: the user's file must end up the
   * same size as the copy, or the marker is left alone and the next tick tries again (a file that
   * was closed, moved or deleted under us says so instead of pretending to have saved).
   */
  private copyOut(copy: string, uri: string, marker: string): boolean {
    const bytes: number = this.sizeOf(copy);
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
    } catch (err) {
      hilog.error(DOMAIN, TAG, 'could not write the document back: %{public}s', JSON.stringify(err));
      return false;
    }
    const arrived: number = this.sizeOf(uri);
    if (arrived >= 0 && arrived !== bytes) {
      hilog.error(DOMAIN, TAG, 'the picked file has %{public}d bytes, the copy %{public}d: writing it again', arrived, bytes);
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
      } catch (err) {
        hilog.error(DOMAIN, TAG, 'the second write also failed: %{public}s', JSON.stringify(err));
        return false;
      }
    }
    this.synced.set(copy, marker);
    hilog.info(DOMAIN, TAG, 'wrote the document back to the picked file (%{public}s bytes)', marker);
    return true;
  }

  /** A file's `size:mtime` marker, or '' when it cannot be read. */
  private markerOf(path: string): string {
    try {
      const info: fileIo.Stat = fileIo.statSync(path);
      return `${info.size}:${info.mtime}`;
    } catch (err) {
      return '';
    }
  }

  /** A file's size in bytes, or -1 when it cannot be read (a sandbox path or a picked URI). */
  private sizeOf(path: string): number {
    try {
      const info: fileIo.Stat = fileIo.statSync(path);
      return info.size;
    } catch (err) {
      return -1;
    }
  }

  /**
   * The app was closed before a change reached the user's file (a save in the last half second, or
   * a kill during the copy). Writing it now is the difference between "your document is safe" and
   * a stale file with newer work hidden in the sandbox.
   */
  private compensate(): void {
    let repaired: number = 0;
    this.targets.forEach((uri: string, copy: string) => {
      const marker: string = this.markerOf(copy);
      if (marker === '' || this.synced.get(copy) === marker) {
        return;
      }
      if (this.copyOut(copy, uri, marker)) {
        repaired++;
      }
    });
    if (repaired > 0) {
      this.saveSynced();
      hilog.info(DOMAIN, TAG, 'wrote back %{public}d document(s) that a previous run did not', repaired);
    }
  }

  /** Remember which copies are already in the user's files (across restarts). */
  private async saveSynced(): Promise<void> {
    if (this.store === undefined) {
      return;
    }
    const table: Record<string, string> = {};
    this.synced.forEach((marker: string, copy: string) => {
      table[copy] = marker;
    });
    try {
      await this.store.put('synced', JSON.stringify(table));
      await this.store.flush();
    } catch (err) {
      hilog.warn(DOMAIN, TAG, 'could not remember which files are up to date: %{public}s', JSON.stringify(err));
    }
  }

    /**
   * A copied picture as a file in the app's cache folder, read through its URI: the picked file's
   * grant covers the URI, not the path under /storage/Users, so pasting the path directly would
   * silently fail. '' for anything that is not a picture (documents keep their path).
   */
  private pictureCopy(uri: string): string {
    const lower: string = uri.toLowerCase();
    const picture: boolean = PICTURE_SUFFIXES.some((suffix: string) => lower.endsWith(suffix));
    if (!picture) {
      return '';
    }
    try {
      const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
      const suffix: string = lower.substring(lower.lastIndexOf('.'));
      const path: string = `${context.cacheDir}/pasted-${Date.now()}${suffix}`;
      const source: fileIo.File = fileIo.openSync(uri, fileIo.OpenMode.READ_ONLY);
      try {
        fileIo.copyFileSync(source.fd, path, 0);
      } finally {
        fileIo.closeSync(source);
      }
      hilog.info(DOMAIN, TAG, 'copied a clipboard picture to %{public}s', path);
      return path;
    } catch (err) {
      hilog.warn(DOMAIN, TAG, 'could not read the copied picture: %{public}s', JSON.stringify(err));
      return '';
    }
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
        const uri: string = record.toPlainText();
        // A copied picture is read through its URI (that is what the grant covers); the engine gets
        // a sandbox copy, because the physical path under /storage/Users is not readable by us.
        const local: string = this.pictureCopy(uri);
        if (local !== '') {
          uris.push(local);
        } else {
          const path: string = this.filePath(uri);
          if (path !== '') {
            uris.push(path);
          }
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

  /** Load the last document and the sandbox-path → picked-URI table, and re-activate the grants. */
  private async loadState(context: common.UIAbilityContext): Promise<void> {
    if (context === undefined) {
      native.ready();
      return;
    }
    try {
      this.store = await preferences.getPreferences(context, { name: 'wordcraft' });
      this.lastDocument = await this.store.get('lastDocument', '') as string;
      const saved: string = await this.store.get('targets', '{}') as string;
      const table: Record<string, string> = JSON.parse(saved) as Record<string, string>;
      const policies: fileShare.PolicyInfo[] = [];
      for (const copy of Object.keys(table)) {
        const uri: string = table[copy];
        this.targets.set(copy, uri);
        policies.push({ uri: uri, operationMode: fileShare.OperationMode.READ_MODE | fileShare.OperationMode.WRITE_MODE });
      }
      const markers: string = await this.store.get('synced', '{}') as string;
      const known: Record<string, string> = JSON.parse(markers) as Record<string, string>;
      for (const copy of Object.keys(known)) {
        this.synced.set(copy, known[copy]);
      }
      if (policies.length > 0) {
        // A persisted grant is not in force until it is activated again after a restart.
        await fileShare.activatePermission(policies);
        hilog.info(DOMAIN, TAG, 're-activated %{public}d persisted file grant(s)', policies.length);
      }
      // Anything the previous run saved but never copied out is put right before the document is
      // reopened, so the user never sees an older file than the one on screen.
      this.compensate();
      hilog.info(DOMAIN, TAG, 'last document: %{public}s', this.lastDocument);
    } catch (err) {
      hilog.warn(DOMAIN, TAG, 'no saved state: %{public}s', JSON.stringify(err));
    }
    // Only now does the app know what was open last time: let it ask for that document.
    native.ready();
  }

  /** Remember the sandbox-path → picked-URI table (so saves still reach the user's file). */
  private async saveTargets(): Promise<void> {
    if (this.store === undefined) {
      return;
    }
    const table: Record<string, string> = {};
    this.targets.forEach((uri: string, copy: string) => {
      table[copy] = uri;
    });
    try {
      await this.store.put('targets', JSON.stringify(table));
      await this.store.flush();
    } catch (err) {
      hilog.warn(DOMAIN, TAG, 'could not remember the picked files: %{public}s', JSON.stringify(err));
    }
  }

  /** Remember which document is open, so a relaunch reopens it. */
  private async saveLastDocument(path: string): Promise<void> {
    if (this.store === undefined) {
      return;
    }
    try {
      await this.store.put('lastDocument', path);
      await this.store.flush();
    } catch (err) {
      hilog.warn(DOMAIN, TAG, 'could not remember the document: %{public}s', JSON.stringify(err));
    }
  }

  /**
   * Ask HarmonyOS to keep the grant for a picked file across restarts (`FILE_ACCESS_PERSIST`); a
   * persisted grant has to be activated again on the next launch (see `loadState`).
   */
  private async persistGrant(uri: string): Promise<void> {
    try {
      const policy: fileShare.PolicyInfo = {
        uri: uri,
        operationMode: fileShare.OperationMode.READ_MODE | fileShare.OperationMode.WRITE_MODE,
      };
      await fileShare.persistPermission([policy]);
      hilog.info(DOMAIN, TAG, 'kept the grant for the picked file');
    } catch (err) {
      // Not fatal: the file still works for this session.
      hilog.warn(DOMAIN, TAG, 'could not keep the grant: %{public}s', JSON.stringify(err));
    }
  }

  /**
   * Keep the input method where the document is: attach it when a caret appears (a soft keyboard
   * and the Chinese IME), move its candidate window to the caret, and detach when there is nothing
   * to type into.
   */
  private followCaret(): void {
    if (native.caretPresent() === 0) {
      this.detachInputMethod();
      this.hideImeField();
      return;
    }
    if (USE_IME_FIELD) {
      this.showImeField();
    } else if (!this.imeAttached) {
      this.attachInputMethod();
    } else {
      this.updateCursor();
    }
  }

  /**
   * Put the composing field on the caret and focus it, which brings the keyboard up. The field is
   * as wide as the page (so a composition has room) and exactly as tall as the caret's line.
   */
  private showImeField(): void {
    const x: number = native.caretX();
    const y: number = native.caretY();
    const height: number = Math.max(12, native.caretHeight());
    const width: number = Math.max(120, this.pageWidthVp() - x);
    this.imeFieldX = x;
    this.imeFieldY = y;
    this.imeFieldHeight = height;
    this.imeFieldWidth = width;
    if (!this.imeFieldShown) {
      this.imeFieldShown = true;
    }
    this.imeFieldValue = IME_FIELD_SENTINEL;
    // Focusing has to wait until the field is on screen: asking for a component that the same frame
    // is still mounting took the whole app down (measured: `HandleAppDied` right after the tap).
    setTimeout(() => {
      try {
        this.getUIContext().getFocusController().requestFocus('imeField');
      } catch (err) {
        // 150003 is "no such node": the field has not been laid out yet; try once more shortly.
        hilog.warn(DOMAIN, TAG, 'composing field not there yet: %{public}s', JSON.stringify(err));
        setTimeout(() => {
          try {
            this.getUIContext().getFocusController().requestFocus('imeField');
          } catch (errToo) {
            hilog.error(DOMAIN, TAG, 'could not focus the composing field: %{public}s', JSON.stringify(errToo));
          }
        }, 150);
      }
    }, 30);
  }

  private hideImeField(): void {
    if (!this.imeFieldShown) {
      return;
    }
    this.imeFieldShown = false;
    this.imeFieldValue = IME_FIELD_SENTINEL;
  }

  /** The page width in vp, from the surface the shell reported. */
  private pageWidthVp(): number {
    const rect: SurfaceRect = this.controller.getXComponentSurfaceRect();
    return rect.surfaceWidth / density();
  }

  /** Attach the input method (the keyboard comes up) and listen for what it sends. */
  private attachInputMethod(): void {
    const cursor = this.cursorInfo();
    // The composition (pinyin before it becomes characters) arrives as preview text, which has to
    // be subscribed before attaching. The document draws committed text; drawing the preview
    // inline is the next step (docs/platform-ohos.md).
    this.ime.on('setPreviewText', (text: string, range: inputMethod.Range) => {
      // Composing text (pinyin before it becomes characters): the document draws it at the caret,
      // underlined, exactly as it does for the desktop's IME input.
      native.imePreview(text);
    });
    this.ime.on('finishTextPreview', () => native.imePreview(''));
    this.ime
      .attach(true, {
        inputAttribute: { textInputType: inputMethod.TextInputType.TEXT, enterKeyType: inputMethod.EnterKeyType.DONE },
        cursorInfo: cursor,
      })
      .then(() => {
        this.imeAttached = true;
        hilog.info(DOMAIN, TAG, 'input method attached at (%{public}d, %{public}d)', cursor.left, cursor.top);
        this.listenInputMethod();
      })
      .catch((err: Error) => {
        hilog.error(DOMAIN, TAG, 'input method attach failed: %{public}s', JSON.stringify(err));
      });
  }

  /** What the input method sends while it is attached: text, deletions, Enter, cursor moves. */
  private listenInputMethod(): void {
    this.ime.on('insertText', (text: string) => this.insertText(text));
    this.ime.on('deleteLeft', (length: number) => this.deleteBackward(length));
    this.ime.on('deleteRight', (length: number) => this.deleteForward(length));
    this.ime.on('sendFunctionKey', (key: inputMethod.FunctionKey) => {
      // Enter, Go, Search and Done all mean "a new line" in a document.
      this.pressKey(KEY_ENTER);
    });
    this.ime.on('moveCursor', (direction: inputMethod.Direction) => {
      if (direction === inputMethod.Direction.CURSOR_LEFT) {
        this.pressKey(KEY_ARROW_LEFT);
      } else if (direction === inputMethod.Direction.CURSOR_RIGHT) {
        this.pressKey(KEY_ARROW_RIGHT);
      } else if (direction === inputMethod.Direction.CURSOR_UP) {
        this.pressKey(KEY_ARROW_UP);
      } else if (direction === inputMethod.Direction.CURSOR_DOWN) {
        this.pressKey(KEY_ARROW_DOWN);
      }
    });
    this.ime.on('sendKeyboardStatus', (status: inputMethod.KeyboardStatus) => {
      if (status === inputMethod.KeyboardStatus.HIDE) {
        this.detachInputMethod();
      }
    });
  }

  /** Stop listening and let go of the input method (no keyboard, no candidate window). */
  private detachInputMethod(): void {
    if (!this.imeAttached) {
      return;
    }
    this.imeAttached = false;
    try {
      this.ime.off('insertText');
      this.ime.off('deleteLeft');
      this.ime.off('deleteRight');
      this.ime.off('sendFunctionKey');
      this.ime.off('moveCursor');
      this.ime.off('setPreviewText');
      this.ime.off('finishTextPreview');
      this.ime.off('sendKeyboardStatus');
    } catch (err) {
      hilog.warn(DOMAIN, TAG, 'input method off: %{public}s', JSON.stringify(err));
    }
    this.ime
      .detach()
      .then(() => hilog.info(DOMAIN, TAG, 'input method detached'))
      .catch((err: Error) => hilog.warn(DOMAIN, TAG, 'input method detach: %{public}s', JSON.stringify(err)));
  }

  /** Tell the input method where the caret is, so its candidate window sits there (pixels). */
  private updateCursor(): void {
    const cursor = this.cursorInfo();
    this.ime.updateCursor(cursor).catch((err: Error) => hilog.warn(DOMAIN, TAG, 'updateCursor: %{public}s', JSON.stringify(err)));
  }

  /** The caret in screen pixels (the input method's units). */
  private cursorInfo(): inputMethod.CursorInfo {
    const d: number = density();
    const left: number = Math.round(native.caretX() * d);
    const top: number = Math.round(native.caretY() * d);
    const height: number = Math.max(1, Math.round(native.caretHeight() * d));
    return { left: left, top: top, width: 1, height: height };
  }

  /** Text from the input method: one commit into the document (one undo step per word). */
  private insertText(text: string): void {
    if (text === '') {
      return;
    }
    native.imeCommit(text);
    this.afterInput();
  }

  /** Backspace, `length` times. */
  private deleteBackward(length: number): void {
    this.pressKey(KEY_BACKSPACE, Math.max(1, length));
    this.afterInput();
  }

  /** Delete, `length` times. */
  private deleteForward(length: number): void {
    this.pressKey(KEY_DELETE, Math.max(1, length));
    this.afterInput();
  }

  /**
   * File › Share: hand the document to HarmonyOS's share sheet. The user's own file is shared when
   * the document came from one (its grant covers reading), otherwise the sandbox copy by file URI.
   */
  private async shareDocument(request: HostRequest): Promise<string> {
    const path: string = request.path ?? '';
    if (path === '') {
      return JSON.stringify({ error: 'there is no document to share' });
    }
    // Make sure the user's file is current before it leaves the app.
    this.copyWorkingCopies();
    const target: string = this.targets.get(path) ?? fileUri.getUriFromPath(path);
    const title: string = path.substring(path.lastIndexOf('/') + 1);
    const data: systemShare.SharedData = new systemShare.SharedData({
      utd: utd.UniformDataType.FILE,
      uri: target,
      title: title,
    });
    const controller: systemShare.ShareController = new systemShare.ShareController(data);
    const context = this.getUIContext().getHostContext() as common.UIAbilityContext;
    await controller.show(context, {
      previewMode: systemShare.SharePreviewMode.DEFAULT,
      selectionMode: systemShare.SelectionMode.SINGLE,
    });
    hilog.info(DOMAIN, TAG, 'share sheet shown for %{public}s', title);
    return JSON.stringify({ ok: true });
  }

  /**
   * The document to open at launch: the URI another app shared with us (the file manager's "open
   * with"), else the working copy that was open when the app was last closed. `cancelled` when
   * there is neither, which is the normal cold start.
   */
  private async startupDocument(): Promise<string> {
    if (this.launchUri !== '') {
      const uri: string = this.launchUri;
      this.launchUri = '';
      const copy: string = this.workingCopy(uri);
      if (copy !== '') {
        return JSON.stringify({ path: copy });
      }
      return JSON.stringify({ error: 'the file that asked to open could not be read' });
    }
    if (this.lastDocument !== '' && this.exists(this.lastDocument)) {
      return JSON.stringify({ path: this.lastDocument });
    }
    return JSON.stringify({ cancelled: true });
  }

  /** Whether a sandbox path still exists (a cache clean-up may have taken it). */
  private exists(path: string): boolean {
    try {
      fileIo.accessSync(path);
      return true;
    } catch (err) {
      return false;
    }
  }

  /** Remember the document the page is showing, so the next launch reopens it. */
  private rememberDocument(): void {
    const path: string = native.documentPath();
    if (path !== '' && path !== this.lastDocument) {
      this.lastDocument = path;
      this.saveLastDocument(path);
    }
  }

  /** The composing text's colour: the theme's text colour, so it reads as page text. */
  private imeFieldColour(): string {
    return this.themeDark ? '#ffe8e8e8' : '#ff202020';
  }

  /** Keep the handle on the caret's end, and hide it when there is nothing to edit. */
  private followCaretHandle(): void {
    const present: boolean = native.caretPresent() === 1;
    if (!present) {
      if (this.handleVisible) {
        this.handleVisible = false;
      }
      return;
    }
    const x: number = native.caretX();
    const y: number = native.caretY();
    const height: number = native.caretHeight();
    const range: boolean = native.selectionRange() === 1;
    if (!this.handleVisible) {
      this.handleVisible = true;
    }
    if (range !== this.handleIsRange) {
      this.handleIsRange = range;
    }
    if (Math.abs(x - this.handleX) > 0.5) {
      this.handleX = x;
    }
    if (Math.abs(y - this.handleY) > 0.5) {
      this.handleY = y;
    }
    if (Math.abs(height - this.handleHeight) > 0.5) {
      this.handleHeight = height;
    }
  }

  /**
   * Drag the selection's end. The first event presses with Shift at the handle, which the page
   * treats as "extend the selection to here" (the anchor stays at the other end), and the moves
   * carry the finger on, so the page selects what the finger passes over.
   */
  private dragHandle(event: TouchEvent): void {
    const touch: TouchObject | undefined =
      event.touches.length > 0 ? event.touches[0] : (event.changedTouches.length > 0 ? event.changedTouches[0] : undefined);
    if (touch === undefined) {
      return;
    }
    // The handle's own touch coordinates are local to its 44 vp box; the page needs window ones.
    const x: number = touch.windowX;
    const y: number = touch.windowY;
    if (event.type === TouchType.Down) {
      this.pressDragged = true; // a handle drag is never a context-menu press
      this.stopPressTimer();
      native.pointer(POINTER_DOWN, x, y, BUTTON_NONE, MOD_SHIFT);
    } else if (event.type === TouchType.Move) {
      native.pointer(POINTER_MOVE, x, y, BUTTON_NONE, MOD_SHIFT);
      // Near an edge, scroll a little per event so a selection can run past the screen.
      const heightVp: number = this.surfaceHeightVp();
      if (y < EDGE_SCROLL) {
        native.scroll(0, -EDGE_SCROLL_STEP);
      } else if (heightVp > 0 && y > heightVp - EDGE_SCROLL) {
        native.scroll(0, EDGE_SCROLL_STEP);
      }
    } else if (event.type === TouchType.Up || event.type === TouchType.Cancel) {
      native.pointer(POINTER_UP, x, y, BUTTON_NONE, MOD_SHIFT);
    }
  }

  /** The page height in vp, for the handle's edge scrolling. */
  private surfaceHeightVp(): number {
    const rect: SurfaceRect = this.controller.getXComponentSurfaceRect();
    return rect.surfaceHeight / density();
  }

  /**
   * Two fingers drag the page and pinch it.
   *
   * The platform's own `PinchGesture`/`PanGesture` are deliberately not used: once one of them is
   * attached to the `XComponent`, ArkUI withholds the single-finger touch stream from `onTouch`
   * while it decides, so a slow drag could not be told from a press (measured on the tablet: the
   * long-press menu opened in the middle of a selection drag, and the drag never reached the page).
   * Reading the two touches that are already in `TouchEvent.touches` keeps text selection exact and
   * costs a handful of lines.
   */
  private onMultiTouch(event: TouchEvent): void {
    if (!this.multiTouch) {
      // The first finger had started a caret drag or a selection; take it back.
      this.multiTouch = true;
      this.stopPressTimer();
      native.pointer(POINTER_CANCEL, 0, 0, BUTTON_NONE, 0);
      this.pinchStart = this.fingerSpan(event.touches);
      this.pinchApplied = 1;
      const mid: FingerMid = this.fingerMid(event.touches);
      this.panMidX = mid.x;
      this.panMidY = mid.y;
      this.panVelocityX = 0;
      this.panVelocityY = 0;
      this.panMovedAt = Date.now();
      return;
    }
    // A pinch: the spread between the fingers, against the spread it started with.
    const span: number = this.fingerSpan(event.touches);
    if (this.pinchStart > 8 && span > 8 && this.pinchApplied > 0) {
      const scale: number = span / this.pinchStart;
      if (Math.abs(scale - this.pinchApplied) > 0.005) {
        native.zoom(scale / this.pinchApplied);
        this.pinchApplied = scale;
      }
    }
    // A two-finger scroll: the midpoint's movement, the opposite way (fingers down = page up).
    const mid: FingerMid = this.fingerMid(event.touches);
    const dx: number = mid.x - this.panMidX;
    const dy: number = mid.y - this.panMidY;
    if (dx !== 0 || dy !== 0) {
      native.scroll(-dx, -dy);
      const now: number = Date.now();
      const dt: number = Math.max(1, now - this.panMovedAt) / 1000;
      // Smoothed so one jittery sample cannot throw the page across the screen.
      this.panVelocityX = 0.6 * this.panVelocityX + 0.4 * (-dx / dt);
      this.panVelocityY = 0.6 * this.panVelocityY + 0.4 * (-dy / dt);
      this.panMovedAt = now;
      this.panMidX = mid.x;
      this.panMidY = mid.y;
    }
  }

  /** The fingers are gone: fling the page if it was still moving. */
  private endMultiTouch(): void {
    this.multiTouch = false;
    this.pinchStart = 0;
    this.pinchApplied = 1;
    const idle: number = Date.now() - this.panMovedAt;
    if (idle < 120) {
      native.scrollFling(this.panVelocityX, this.panVelocityY);
    }
    this.panVelocityX = 0;
    this.panVelocityY = 0;
  }

  /** The distance between the first two fingers (vp). */
  private fingerSpan(touches: TouchObject[]): number {
    if (touches.length < 2) {
      return 0;
    }
    const dx: number = touches[1].x - touches[0].x;
    const dy: number = touches[1].y - touches[0].y;
    return Math.sqrt(dx * dx + dy * dy);
  }

  /** The midpoint of the first two fingers (vp). */
  private fingerMid(touches: TouchObject[]): FingerMid {
    if (touches.length < 2) {
      return { x: 0, y: 0 };
    }
    return { x: (touches[0].x + touches[1].x) / 2, y: (touches[0].y + touches[1].y) / 2 };
  }

  /**
   * A finger that stays still for [`LONG_PRESS_MS`] opens the document's context menu where it is,
   * which is what a long press means on every touch platform. The platform's own
   * `LongPressGesture` is deliberately not used: a recognised gesture takes the touch stream away
   * from `onTouch`, and then a slow drag can no longer be told from a press (measured: the menu
   * opened in the middle of a selection drag).
   */
  private startPressTimer(x: number, y: number): void {
    this.pressX = x;
    this.pressY = y;
    this.pressDragged = false;
    this.stopPressTimer();
    this.pressTimer = setTimeout(() => {
      this.pressTimer = -1;
      if (this.pressDragged || this.multiTouch) {
        return;
      }
      // The lift that ends this press belongs to the menu it opens, not to the page.
      this.afterLongPress = true;
      native.pointer(POINTER_DOWN, this.pressX, this.pressY, BUTTON_SECONDARY, 0);
      native.pointer(POINTER_UP, this.pressX, this.pressY, BUTTON_SECONDARY, 0);
    }, LONG_PRESS_MS);
  }

  private stopPressTimer(): void {
    if (this.pressTimer !== -1) {
      clearTimeout(this.pressTimer);
      this.pressTimer = -1;
    }
  }

  /** A hardware-style key press: down, then up. */
  private pressKey(keyCode: number, times: number = 1): void {
    for (let i = 0; i < times; i++) {
      native.key(keyCode, true, false, 0, '');
      native.key(keyCode, false, false, 0, '');
    }
  }

  /** The document moved the caret: keep the candidate window with it. */
  private afterInput(): void {
    if (native.caretPresent() === 0) {
      this.detachInputMethod();
    } else if (this.imeAttached) {
      this.updateCursor();
    }
  }

  private stopFrames(): void {
    if (this.sync !== undefined) {
      this.sync.stop();
      this.sync = undefined;
    }
  }

  build() {
    Stack({ alignContent: Alignment.TopStart }) {
      XComponent({ type: XComponentType.SURFACE, controller: this.controller })
        .id('wordcraft')
        .width('100%')
        .height('100%')
        .focusable(true)
        .focusOnTouch(true)
        .defaultFocus(true)
        .onFocus(() => native.focus(true))
        .onBlur(() => {
          native.focus(false);
          this.detachInputMethod();
        })
        .onTouch((event: TouchEvent) => {
          // A mouse click arrives here as well as in `onMouse`: two pointer events for one click
          // make egui read a double click and select a word at random.
          if (event.sourceTool === SourceTool.MOUSE) {
            return;
          }
          // Two or more fingers are a pinch or a scroll (the gestures below handle them), never a
          // selection: the page must not see them as a pointer at all, or the selection would jump.
          this.touching = event.touches.length;
          if (this.touching >= 2) {
            this.onMultiTouch(event);
            return;
          }
          if (this.multiTouch) {
            if (this.touching === 0) {
              // Every finger is up: throw the page on if it was still moving, and go back to
              // single-finger editing.
              this.endMultiTouch();
            }
            return;
          }
          if (this.afterLongPress) {
            // The lift that ends a long press belongs to the menu it opened, not to the page.
            if (event.type === TouchType.Up || event.type === TouchType.Cancel) {
              this.afterLongPress = false;
            }
            return;
          }
          // `touches` is empty on Up/Cancel in some versions; the changed touch is always there.
          const touch: TouchObject | undefined =
            event.touches.length > 0 ? event.touches[0] : (event.changedTouches.length > 0 ? event.changedTouches[0] : undefined);
          if (touch === undefined) {
            return;
          }
          let kind: number = POINTER_MOVE;
          if (event.type === TouchType.Down) {
            kind = POINTER_DOWN;
            this.startPressTimer(touch.x, touch.y);
          } else if (event.type === TouchType.Move) {
            // 8 vp of travel means the finger is dragging (a selection), not pressing.
            if (Math.abs(touch.x - this.pressX) + Math.abs(touch.y - this.pressY) > 8) {
              this.pressDragged = true;
              this.stopPressTimer();
            }
          } else if (event.type === TouchType.Up) {
            kind = POINTER_UP;
            this.stopPressTimer();
          } else if (event.type === TouchType.Cancel) {
            kind = POINTER_CANCEL;
            this.stopPressTimer();
          }
          native.pointer(kind, touch.x, touch.y, BUTTON_NONE, 0);
          // A tap that put a caret in the document is a request to type: bring up the keyboard.
          if (kind === POINTER_UP) {
            this.followCaret();
          }
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

      // The composing field: invisible but for the IME's own text, sitting on the caret's line.
      if (this.imeFieldShown) {
        // `$$` both ways: the field shows what the state says, so clearing it after a commit
        // really empties it (a one-way `text` kept the typed characters and every commit then
        // carried the whole accumulated string).
        TextInput({ text: $$this.imeFieldValue, controller: this.imeField })
          .id('imeField')
          .width(this.imeFieldWidth)
          .height(this.imeFieldHeight)
          .position({ x: this.imeFieldX, y: this.imeFieldY })
          .fontSize(Math.max(10, this.imeFieldHeight * 0.78))
          .fontColor(this.imeFieldColour())
          .caretColor('#00000000')
          .backgroundColor('#00000000')
          .borderWidth(0)
          .padding(0)
          .maxLines(1)
          .enablePreviewText(true)
          .enterKeyType(EnterKeyType.Done)
          .onChange((value: string) => {
            // Only committed text lands in `value` (the composition is preview). The field holds a
            // zero-width sentinel so a Backspace from the keyboard deletes *something*: an empty
            // field swallows it and the document never hears about the delete.
            if (value === IME_FIELD_SENTINEL) {
              return;
            }
            if (value === '') {
              // The sentinel itself was deleted: that is Backspace.
              this.pressKey(KEY_BACKSPACE);
            } else if (value.startsWith(IME_FIELD_SENTINEL)) {
              const text: string = value.substring(IME_FIELD_SENTINEL.length);
              if (text !== '') {
                native.imeCommit(text);
              }
            } else {
              // Typed over the sentinel (a paste or an IME that replaces it): the whole value is new.
              native.imeCommit(value);
            }
            this.imeFieldValue = IME_FIELD_SENTINEL;
          })
          .onSubmit(() => this.pressKey(KEY_ENTER))
          .onKeyEvent((event: KeyEvent) => {
            if (event.type !== KeyType.Down && event.type !== KeyType.Up) {
              return false;
            }
            // Hardware keys belong to the document (shortcuts, arrows, Backspace), not the field.
            native.key(event.keyCode, event.type === KeyType.Down, false, modifierBits(event), '');
            return true;
          })
      }

      // The selection handle: a dot under the caret's end, on a finger-sized hit area. Dragging it
      // extends the selection from the other end (a Shift press keeps the anchor), and near the top
      // or bottom edge it scrolls the page, so a selection can run past the screen.
      if (this.handleVisible) {
        Stack() {
          Circle()
            .width(this.handleIsRange ? 18 : 14)
            .height(this.handleIsRange ? 18 : 14)
            .fill('#ff2d6ae0')
            .stroke('#ffffffff')
            .strokeWidth(2)
            .hitTestBehavior(HitTestMode.None)
        }
        .width(HANDLE_TOUCH)
        .height(HANDLE_TOUCH)
        .position({
          x: this.handleX - HANDLE_TOUCH / 2,
          y: this.handleY + this.handleHeight - HANDLE_TOUCH / 2,
        })
        .onTouch((event: TouchEvent) => this.dragHandle(event))
      }
    }
    .width('100%')
    .height('100%')
  }
}
