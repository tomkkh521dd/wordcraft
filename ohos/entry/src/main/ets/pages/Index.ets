import native from 'libwordcraft.so';
import { display, font, window } from '@kit.ArkUI';
import { displaySync } from '@kit.ArkGraphics2D';
import { common, ConfigurationConstant } from '@kit.AbilityKit';
import { i18n } from '@kit.LocalizationKit';
import { hilog } from '@kit.PerformanceAnalysisKit';

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

@Entry
@Component
struct Index {
  private controller: WordcraftController = new WordcraftController();
  private sync: displaySync.DisplaySync | undefined = undefined;
  private presented: number = 0;
  private started: boolean = false;

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
  }

  aboutToDisappear(): void {
    this.stopFrames();
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
      if (native.render() === 1) {
        this.presented++;
        if (this.presented % 300 === 0) {
          hilog.info(DOMAIN, TAG, 'stats %{public}s', native.stats());
        }
      }
    });
    this.sync.start();
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
